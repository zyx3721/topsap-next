//! 隧道层：一条通往网关的 TLS 长连接，之上承载 16 字节头的自定义帧。
//!
//! 握手序列（实测）：
//! ```text
//! TLS 建连（不校验证书、不发 SNI）
//! → CONNECT 前言
//! → 登录帧（type=1，载荷携带 session_id）
//! ← 确认帧（type=2）
//! → 通知帧（type=7，8 字节零）
//! ↔ 数据帧（type=15，原始 IPv4）
//! ```

use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_rustls::client::TlsStream;
use tokio_util::sync::CancellationToken;

use crate::auth::LoginSession;
use crate::protocol::{self, Frame, FrameParser, MSG_ACK, MSG_DATA};
use crate::tls;

mod handshake;

pub use handshake::HandshakeInfo;
use handshake::ParsedHandshake;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
const WRITE_TIMEOUT: Duration = Duration::from_secs(15);
const OUTBOUND_QUEUE: usize = 512;
pub const EVENT_QUEUE: usize = 2048;

const CONNECT_PREFACE: &str = "CONNECT 127.0.0.1:4430 HTTP/1.1\r\n\
host:127.0.0.1\r\n\
User-Agent: VONE-NA,WINDOWS,NULL,TOPCLINET,NULL\r\n\
Proxy-Connection: Keep-Alive\r\n\r\n";

#[derive(Debug, thiserror::Error)]
pub enum TunnelError {
    #[error("{0}")]
    Tls(#[from] tls::TlsError),
    #[error("隧道握手超时：{0} 秒内未收到登录确认")]
    AckTimeout(u64),
    #[error("隧道握手失败：{0}")]
    Handshake(String),
    /// 网关通过握手回应或错误通告明确拒绝，不能按普通断线自动重连
    #[error("{0}")]
    Rejected(String),
    /// 网关判定会话已失效（强制注销、被踢出、超时等），必须重新登录
    #[error("{0}")]
    SessionExpired(String),
    #[error("隧道读写失败：{0}")]
    Io(String),
    #[error("{0}")]
    Protocol(#[from] protocol::ProtoError),
}

/// 隧道向引擎上报的事件
#[derive(Debug, Clone)]
pub enum TunnelEvent {
    /// 握手完成，可以收发数据帧
    Ready,
    /// 网关下发的 IPv4 报文
    Data(Vec<u8>),
    /// 数据帧载荷不是 IPv4，无法注入系统网卡
    NonIpv4Data { len: usize },
    /// 非数据帧（控制/心跳），仅用于日志
    Control { msg_type: u32, payload: Vec<u8> },
    /// 网关明确拒绝隧道，必须断开而不是自动重连
    Rejected {
        reason: String,
        /// 会话级失效：继续复用 session_id 重连没有意义
        session_expired: bool,
    },
    /// 连接结束
    Closed { reason: String },
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TunnelStats {
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub packets_up: u64,
    pub packets_down: u64,
}

#[derive(Clone)]
pub struct TunnelHandle {
    outbound: mpsc::Sender<Vec<u8>>,
    cancel: CancellationToken,
    bytes_up: Arc<AtomicU64>,
    bytes_down: Arc<AtomicU64>,
    packets_up: Arc<AtomicU64>,
    packets_down: Arc<AtomicU64>,
    handshake: Option<HandshakeInfo>,
}

impl TunnelHandle {
    /// 发送一个 IPv4 报文。队列满时丢弃并返回 false（VPN 场景宁可丢包也不阻塞）。
    pub fn send_packet(&self, packet: Vec<u8>) -> bool {
        self.outbound.try_send(packet).is_ok()
    }

    pub fn close(&self) {
        self.cancel.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// 网关握手回应中解析到的隧道侧网络参数；旧网关可能不返回
    pub fn handshake(&self) -> Option<&HandshakeInfo> {
        self.handshake.as_ref()
    }

    pub fn stats(&self) -> TunnelStats {
        TunnelStats {
            bytes_up: self.bytes_up.load(Ordering::Relaxed),
            bytes_down: self.bytes_down.load(Ordering::Relaxed),
            packets_up: self.packets_up.load(Ordering::Relaxed),
            packets_down: self.packets_down.load(Ordering::Relaxed),
        }
    }
}

/// 建立隧道：TLS 建连 → 登录帧 → 等 ACK → 通知帧 → 启动双向任务。
pub async fn open(
    session: &LoginSession,
    events: mpsc::Sender<TunnelEvent>,
    parent: CancellationToken,
) -> Result<TunnelHandle, TunnelError> {
    let (stream, _ip) = tls::connect(&session.gateway, session.port).await?;
    finish_open(stream, session, events, parent).await
}

/// 使用已解析的网关 IP 建立隧道，避免重连时重复解析域名
pub async fn open_at(
    session: &LoginSession,
    events: mpsc::Sender<TunnelEvent>,
    parent: CancellationToken,
    ip: IpAddr,
) -> Result<TunnelHandle, TunnelError> {
    let stream = tls::connect_ip(ip, session.port).await?;
    finish_open(stream, session, events, parent).await
}

async fn finish_open(
    mut stream: TlsStream<TcpStream>,
    session: &LoginSession,
    events: mpsc::Sender<TunnelEvent>,
    parent: CancellationToken,
) -> Result<TunnelHandle, TunnelError> {
    let (parser, prefetched, handshake) = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        run_handshake(&mut stream, &session.session_id),
    )
    .await
    .map_err(|_| TunnelError::AckTimeout(HANDSHAKE_TIMEOUT.as_secs()))??;

    let (read_half, write_half) = tokio::io::split(stream);
    let (outbound_tx, outbound_rx) = mpsc::channel::<Vec<u8>>(OUTBOUND_QUEUE);
    let cancel = parent.child_token();

    let bytes_up = Arc::new(AtomicU64::new(0));
    let bytes_down = Arc::new(AtomicU64::new(0));
    let packets_up = Arc::new(AtomicU64::new(0));
    let packets_down = Arc::new(AtomicU64::new(0));

    spawn_reader(
        read_half,
        parser,
        prefetched,
        events.clone(),
        cancel.clone(),
        bytes_down.clone(),
        packets_down.clone(),
    );
    spawn_writer(
        write_half,
        outbound_rx,
        events.clone(),
        cancel.clone(),
        bytes_up.clone(),
        packets_up.clone(),
    );

    let _ = events.send(TunnelEvent::Ready).await;

    Ok(TunnelHandle {
        outbound: outbound_tx,
        cancel,
        bytes_up,
        bytes_down,
        packets_up,
        packets_down,
        handshake,
    })
}

async fn run_handshake(
    stream: &mut TlsStream<TcpStream>,
    session_id: &str,
) -> Result<(FrameParser, Vec<Frame>, Option<HandshakeInfo>), TunnelError> {
    stream
        .write_all(CONNECT_PREFACE.as_bytes())
        .await
        .map_err(|e| TunnelError::Io(format!("发送 CONNECT 前言失败：{e}")))?;
    let login = protocol::build_login_frame(session_id)?;
    stream
        .write_all(&login)
        .await
        .map_err(|e| TunnelError::Io(format!("发送登录帧失败：{e}")))?;
    stream
        .flush()
        .await
        .map_err(|e| TunnelError::Io(format!("刷新登录帧失败：{e}")))?;

    let mut parser = FrameParser::new();
    let mut buf = vec![0u8; 16 * 1024];
    let mut ack_seen = false;
    let mut info: Option<HandshakeInfo> = None;
    let mut before_ack = Vec::new();
    let mut after_ack = Vec::new();

    while !ack_seen {
        let n = stream
            .read(&mut buf)
            .await
            .map_err(|e| TunnelError::Io(format!("读取握手响应失败：{e}")))?;
        if n == 0 {
            return Err(TunnelError::Handshake(
                "网关在发送登录确认前关闭了连接".to_string(),
            ));
        }
        for frame in parser.push(&buf[..n])? {
            if ack_seen {
                after_ack.push(frame);
                continue;
            }
            if frame.msg_type == MSG_ACK {
                ack_seen = true;
                match handshake::parse_response(&frame.payload) {
                    ParsedHandshake::GatewayError { code, message } => {
                        return Err(rejection_for(code, message));
                    }
                    ParsedHandshake::Ok(parsed) => info = Some(parsed),
                    ParsedHandshake::Unknown => {}
                }
                continue;
            }
            if let Some(error) = handshake::classify_error(&frame.payload) {
                return Err(rejection_error(error));
            }
            match handshake::parse_response(&frame.payload) {
                ParsedHandshake::Ok(parsed) => {
                    if info.is_none() {
                        info = Some(parsed);
                    }
                    before_ack.push(frame);
                }
                ParsedHandshake::GatewayError { code, message } => {
                    return Err(rejection_for(code, message));
                }
                ParsedHandshake::Unknown => before_ack.push(frame),
            }
        }
    }

    before_ack.append(&mut after_ack);
    let prefetched = before_ack;
    for frame in &prefetched {
        if let Some(error) = handshake::classify_error(&frame.payload) {
            return Err(rejection_error(error));
        }
        if info.is_none() {
            if let ParsedHandshake::Ok(parsed) = handshake::parse_response(&frame.payload) {
                info = Some(parsed);
            }
        }
    }

    stream
        .write_all(&protocol::build_notify_frame())
        .await
        .map_err(|e| TunnelError::Io(format!("发送通知帧失败：{e}")))?;
    stream
        .flush()
        .await
        .map_err(|e| TunnelError::Io(format!("刷新通知帧失败：{e}")))?;

    Ok((parser, prefetched, info))
}

/// 网关错误码转换为隧道错误：会话级失效单独上报，避免继续做无意义的隧道重连
fn rejection_for(code: u32, message: String) -> TunnelError {
    if handshake::is_session_expired(code) {
        TunnelError::SessionExpired(message)
    } else {
        TunnelError::Rejected(message)
    }
}

fn rejection_error(error: handshake::GatewayError) -> TunnelError {
    rejection_for(error.code, error.message)
}

fn spawn_reader(
    mut read_half: tokio::io::ReadHalf<TlsStream<TcpStream>>,
    mut parser: FrameParser,
    prefetched: Vec<Frame>,
    events: mpsc::Sender<TunnelEvent>,
    cancel: CancellationToken,
    bytes_down: Arc<AtomicU64>,
    packets_down: Arc<AtomicU64>,
) {
    tokio::spawn(async move {
        for frame in prefetched {
            if !handle_frame(frame, &events, &bytes_down, &packets_down).await {
                return;
            }
        }

        let mut buf = vec![0u8; 64 * 1024];
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = events.send(TunnelEvent::Closed { reason: "本地主动关闭".to_string() }).await;
                    return;
                }
                result = read_half.read(&mut buf) => {
                    match result {
                        Ok(0) => {
                            let _ = events.send(TunnelEvent::Closed { reason: "网关关闭了隧道连接".to_string() }).await;
                            return;
                        }
                        Ok(n) => {
                            match parser.push(&buf[..n]) {
                                Ok(frames) => {
                                    for frame in frames {
                                        if !handle_frame(frame, &events, &bytes_down, &packets_down).await {
                                            return;
                                        }
                                    }
                                }
                                Err(err) => {
                                    let _ = events.send(TunnelEvent::Closed { reason: format!("帧解析失败：{err}") }).await;
                                    return;
                                }
                            }
                        }
                        Err(err) => {
                            let _ = events.send(TunnelEvent::Closed { reason: format!("读取隧道失败：{err}") }).await;
                            return;
                        }
                    }
                }
            }
        }
    });
}

async fn handle_frame(
    frame: Frame,
    events: &mpsc::Sender<TunnelEvent>,
    bytes_down: &Arc<AtomicU64>,
    packets_down: &Arc<AtomicU64>,
) -> bool {
    if frame.msg_type == MSG_DATA {
        if !protocol::is_ipv4(&frame.payload) {
            return events
                .send(TunnelEvent::NonIpv4Data {
                    len: frame.payload.len(),
                })
                .await
                .is_ok();
        }
        bytes_down.fetch_add(frame.payload.len() as u64, Ordering::Relaxed);
        packets_down.fetch_add(1, Ordering::Relaxed);
        return events.send(TunnelEvent::Data(frame.payload)).await.is_ok();
    }

    if let Some(error) = handshake::classify_error(&frame.payload) {
        let session_expired = error.session_expired();
        return events
            .send(TunnelEvent::Rejected {
                reason: error.message,
                session_expired,
            })
            .await
            .is_ok();
    }

    events
        .send(TunnelEvent::Control {
            msg_type: frame.msg_type,
            payload: frame.payload,
        })
        .await
        .is_ok()
}

fn spawn_writer(
    mut write_half: tokio::io::WriteHalf<TlsStream<TcpStream>>,
    mut outbound: mpsc::Receiver<Vec<u8>>,
    events: mpsc::Sender<TunnelEvent>,
    cancel: CancellationToken,
    bytes_up: Arc<AtomicU64>,
    packets_up: Arc<AtomicU64>,
) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                packet = outbound.recv() => {
                    let Some(packet) = packet else { return };
                    let frame = protocol::build_data_frame(&packet);
                    let result = tokio::select! {
                        _ = cancel.cancelled() => return,
                        result = tokio::time::timeout(WRITE_TIMEOUT, async {
                            write_half.write_all(&frame).await?;
                            write_half.flush().await
                        }) => result,
                    };
                    match result {
                        Ok(Ok(())) => {
                            bytes_up.fetch_add(packet.len() as u64, Ordering::Relaxed);
                            packets_up.fetch_add(1, Ordering::Relaxed);
                        }
                        Ok(Err(err)) => {
                            let reason = format!("写入隧道失败：{err}");
                            let _ = events.send(TunnelEvent::Closed { reason }).await;
                            return;
                        }
                        Err(_) => {
                            let reason = format!("写入隧道超时（{} 秒）", WRITE_TIMEOUT.as_secs());
                            let _ = events.send(TunnelEvent::Closed { reason }).await;
                            return;
                        }
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 统计初值为零() {
        let stats = TunnelStats::default();
        assert_eq!(stats.bytes_up, 0);
        assert_eq!(stats.packets_down, 0);
    }
}
