//! 内网 DNS 主动探活：通过隧道向网关下发的 DNS 发送标准查询，验证下行是否仍在转发。

use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use tokio::net::UdpSocket;

const QUERY_TIMEOUT: Duration = Duration::from_secs(3);
/// 门户资源里取不到域名时使用的查询名；只用于触发 DNS 响应，不访问该站点
pub const FALLBACK_NAME: &str = "www.example.com";
const MAX_RESPONSE: usize = 512;

pub struct DnsProbe {
    socket: UdpSocket,
    server: SocketAddr,
    name: String,
    next_id: u16,
}

impl DnsProbe {
    /// 绑定虚拟 IP；绑定失败时不启用探活，避免网关丢弃非虚拟 IP 源地址的探测包
    pub async fn bind(local: IpAddr, server: SocketAddr, name: String) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(SocketAddr::new(local, 0)).await?;
        Ok(Self {
            socket,
            server,
            name,
            next_id: rand::random(),
        })
    }

    /// 发送一次 A 记录查询；任意合法响应（含 NXDOMAIN）都视为下行可用
    pub async fn query(&mut self) -> bool {
        self.query_with_timeout(QUERY_TIMEOUT).await
    }

    async fn query_with_timeout(&mut self, timeout: Duration) -> bool {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        let packet = build_query(id, &self.name);
        if self.socket.send_to(&packet, self.server).await.is_err() {
            return false;
        }

        let deadline = tokio::time::Instant::now() + timeout;
        let mut buf = [0u8; MAX_RESPONSE];
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return false;
            }
            match tokio::time::timeout(remaining, self.socket.recv_from(&mut buf)).await {
                Ok(Ok((len, _))) => {
                    if is_response(&buf[..len], id) {
                        return true;
                    }
                }
                _ => return false,
            }
        }
    }
}

/// 构造一个标准 DNS A 记录查询包
pub fn build_query(id: u16, name: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + name.len() + 6);
    out.extend_from_slice(&id.to_be_bytes());
    out.extend_from_slice(&[0x01, 0x00]);
    out.extend_from_slice(&[0x00, 0x01]);
    out.extend_from_slice(&[0x00, 0x00]);
    out.extend_from_slice(&[0x00, 0x00]);
    out.extend_from_slice(&[0x00, 0x00]);
    for label in name.split('.') {
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    out.extend_from_slice(&[0x00, 0x01]);
    out.extend_from_slice(&[0x00, 0x01]);
    out
}

/// 响应 ID 匹配且 QR 位为 1；不要求 rcode 成功，NXDOMAIN 同样说明链路可用
pub fn is_response(buf: &[u8], id: u16) -> bool {
    buf.len() >= 12 && u16::from_be_bytes([buf[0], buf[1]]) == id && buf[2] & 0x80 != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 查询包为单个_a_记录() {
        let packet = build_query(0x1234, "vpn.example.com");
        assert_eq!(&packet[0..2], &[0x12, 0x34]);
        assert_eq!(&packet[4..6], &[0x00, 0x01]);
        assert_eq!(&packet[2..4], &[0x01, 0x00]);
        assert_eq!(&packet[packet.len() - 4..], &[0x00, 0x01, 0x00, 0x01]);
        assert!(packet.windows(3).any(|w| w == b"vpn"));
    }

    #[test]
    fn 响应匹配要求_id_与_qr_位() {
        let mut reply = vec![0u8; 12];
        reply[0] = 0x12;
        reply[1] = 0x34;
        reply[2] = 0x80;
        assert!(is_response(&reply, 0x1234));
        assert!(!is_response(&reply, 0x1235));
        reply[2] = 0x00;
        assert!(!is_response(&reply, 0x1234));
        assert!(!is_response(&reply[..11], 0x1234));
    }

    #[tokio::test]
    async fn 收到响应判定成功() {
        let server = UdpSocket::bind("127.0.0.1:0").await.expect("绑定测试服务");
        let address = server.local_addr().expect("取得测试地址");
        tokio::spawn(async move {
            let mut buf = [0u8; MAX_RESPONSE];
            let (len, peer) = server.recv_from(&mut buf).await.expect("收到查询");
            let mut reply = buf[..len].to_vec();
            reply[2] |= 0x80;
            server.send_to(&reply, peer).await.expect("发送响应");
        });

        let mut probe = DnsProbe::bind(
            "127.0.0.1".parse().expect("本地地址"),
            address,
            FALLBACK_NAME.to_string(),
        )
        .await
        .expect("绑定探活套接字");
        assert!(probe.query_with_timeout(Duration::from_secs(2)).await);
    }

    #[tokio::test]
    async fn 超时判定失败() {
        let server = UdpSocket::bind("127.0.0.1:0").await.expect("绑定测试服务");
        let address = server.local_addr().expect("取得测试地址");
        let mut probe = DnsProbe::bind(
            "127.0.0.1".parse().expect("本地地址"),
            address,
            FALLBACK_NAME.to_string(),
        )
        .await
        .expect("绑定探活套接字");
        assert!(!probe.query_with_timeout(Duration::from_millis(50)).await);
    }
}
