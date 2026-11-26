//! LoginServer 握手回应解析：识别网关错误通告并提取隧道侧网络参数。

use std::net::Ipv4Addr;

use serde_json::Value;

/// 官方 na_client.exe 的 ProcessErrorPacket 只处理 39..=57 这一区间
const ERROR_CODE_MIN: u32 = 39;
const ERROR_CODE_MAX: u32 = 57;
/// 二进制回应中虚拟地址/掩码所在偏移（相对载荷起始）
const BINARY_VIP_OFFSET: usize = 0x10;
const BINARY_MASK_OFFSET: usize = 0x14;
/// 官方客户端固定读取 14 个 32 位字段，载荷不足时不做地址判断
const BINARY_MIN_LEN: usize = 0x38;

/// 握手回应中由网关下发的网络参数
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HandshakeInfo {
    pub vip: Option<Ipv4Addr>,
    pub mask: Option<Ipv4Addr>,
    pub dns1: Option<Ipv4Addr>,
    pub dns2: Option<Ipv4Addr>,
    pub req_packet: Option<u32>,
}

/// 握手回应的解析结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedHandshake {
    Ok(HandshakeInfo),
    GatewayError { code: u32, message: String },
    Unknown,
}

/// 网关错误通告：错误码与可读原因
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayError {
    pub code: u32,
    pub message: String,
}

impl GatewayError {
    /// 会话已失效：继续复用 session_id 重连没有意义，必须重新登录
    pub fn session_expired(&self) -> bool {
        is_session_expired(self.code)
    }
}

/// 会话级失效错误码：39 会话 ID 非法、42 被踢出、50 连接超时、52 重连虚 IP 无效、57 账号受限
pub fn is_session_expired(code: u32) -> bool {
    matches!(code, 39 | 42 | 50 | 52 | 57)
}

/// 解析 ACK 载荷；无法识别时返回 Unknown，保持对旧网关的兼容
pub fn parse_response(payload: &[u8]) -> ParsedHandshake {
    if payload.is_empty() {
        return ParsedHandshake::Unknown;
    }
    let trimmed = trim_ascii_start(payload);
    if matches!(trimmed.first(), Some(b'{') | Some(b'[')) {
        return match serde_json::from_slice::<Value>(trimmed) {
            Ok(value) => parse_json(&value),
            // 看起来像 JSON 但解析失败时不再按二进制猜测，避免误读字段
            Err(_) => ParsedHandshake::Unknown,
        };
    }
    parse_binary(payload)
}

/// 只识别明确的错误通告，用于 ACK 之外的控制帧，避免把普通帧误判为地址错误
pub fn classify_error(payload: &[u8]) -> Option<GatewayError> {
    if payload.is_empty() {
        return None;
    }
    let trimmed = trim_ascii_start(payload);
    if matches!(trimmed.first(), Some(b'{') | Some(b'[')) {
        let value = serde_json::from_slice::<Value>(trimmed).ok()?;
        return req_packet(&value)
            .filter(|code| is_error_code(*code))
            .map(gateway_error);
    }
    if payload.len() < 4 {
        return None;
    }
    let code = u32::from_be_bytes(payload[0..4].try_into().ok()?);
    is_error_code(code).then(|| gateway_error(code))
}

/// 网关错误码对应的用户可读文案
pub fn gateway_error_text(code: u32) -> Option<&'static str> {
    match code {
        39 => Some("网关判定会话 ID 无效"),
        40 => Some("网关获取虚拟地址池失败"),
        41 => Some("网关获取 ACL 规则失败"),
        42 => Some("账号已被网关踢下线"),
        45 => Some("网关获取 DHCP 地址失败"),
        50 => Some("网关判定客户端连接超时"),
        51 => Some("网关获取 DNS 域名列表失败"),
        52 => Some("重连时的虚拟地址无效"),
        53 => Some("网关繁忙，暂时无法处理请求"),
        56 => Some("网关发生集群切换"),
        57 => Some("登录账号受限，请使用已绑定的账号"),
        _ => None,
    }
}

/// 拼接完整的拒绝原因
pub fn rejection_message(code: u32) -> String {
    match gateway_error_text(code) {
        Some(text) => format!("网关拒绝建立隧道：{text}（错误码 {code}）"),
        None => format!("网关拒绝建立隧道（错误码 {code}）"),
    }
}

fn is_error_code(code: u32) -> bool {
    (ERROR_CODE_MIN..=ERROR_CODE_MAX).contains(&code)
}

fn gateway_error(code: u32) -> GatewayError {
    GatewayError {
        code,
        message: rejection_message(code),
    }
}

fn parse_json(value: &Value) -> ParsedHandshake {
    let code = req_packet(value);
    if let Some(code) = code.filter(|code| is_error_code(*code)) {
        return ParsedHandshake::GatewayError {
            code,
            message: rejection_message(code),
        };
    }

    let packet = value.get("resp_packet").unwrap_or(value);
    let vip = read_ipv4(packet.get("vip_v4"));
    let mask = read_ipv4(packet.get("vip_mask"));
    let dns1 = read_ipv4(packet.get("dns1_v4"));
    let dns2 = read_ipv4(packet.get("dns2_v4"));
    if vip.is_none() && mask.is_none() {
        return ParsedHandshake::Unknown;
    }
    if vip == Some(Ipv4Addr::UNSPECIFIED) || mask == Some(Ipv4Addr::UNSPECIFIED) {
        return ParsedHandshake::GatewayError {
            code: 45,
            message: "网关未分配虚拟地址（地址或掩码为 0），请检查网关地址池".to_string(),
        };
    }
    ParsedHandshake::Ok(HandshakeInfo {
        vip,
        mask,
        dns1,
        dns2,
        req_packet: code,
    })
}

fn parse_binary(payload: &[u8]) -> ParsedHandshake {
    if payload.len() < 4 {
        return ParsedHandshake::Unknown;
    }
    let code = u32::from_be_bytes(payload[0..4].try_into().unwrap());
    if is_error_code(code) {
        return ParsedHandshake::GatewayError {
            code,
            message: rejection_message(code),
        };
    }
    // 首个字段应是类型/长度等小整数；明显不是结构化回应时不再猜测地址布局
    if code > u32::from(u16::MAX) {
        return ParsedHandshake::Unknown;
    }
    if payload.len() < BINARY_MIN_LEN {
        return ParsedHandshake::Unknown;
    }
    let vip = Ipv4Addr::from(u32::from_be_bytes(
        payload[BINARY_VIP_OFFSET..BINARY_VIP_OFFSET + 4]
            .try_into()
            .unwrap(),
    ));
    let mask = Ipv4Addr::from(u32::from_be_bytes(
        payload[BINARY_MASK_OFFSET..BINARY_MASK_OFFSET + 4]
            .try_into()
            .unwrap(),
    ));
    if vip.is_unspecified() || mask.is_unspecified() {
        // 只有回应里确实带了其他结构化字段时才认定「网关明确表示未分配地址」；
        // 全零载荷更可能是空确认帧，按无法识别处理，避免误伤旧网关
        if !has_structural_data(payload) {
            return ParsedHandshake::Unknown;
        }
        return ParsedHandshake::GatewayError {
            code: 45,
            message: "网关未分配虚拟地址（地址或掩码为 0），请检查网关地址池".to_string(),
        };
    }
    if !is_contiguous_mask(mask) {
        return ParsedHandshake::Unknown;
    }
    ParsedHandshake::Ok(HandshakeInfo {
        vip: Some(vip),
        mask: Some(mask),
        dns1: None,
        dns2: None,
        req_packet: Some(code),
    })
}

fn req_packet(value: &Value) -> Option<u32> {
    read_u32(value.get("req_packet")).or_else(|| {
        value
            .get("resp_packet")
            .and_then(|packet| read_u32(packet.get("req_packet")))
    })
}

fn read_u32(value: Option<&Value>) -> Option<u32> {
    match value? {
        Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// 读取 IPv4 文本；缺失或非法返回 None，`0.0.0.0` 会保留为非法的未指定地址
fn read_ipv4(value: Option<&Value>) -> Option<Ipv4Addr> {
    value?.as_str()?.trim().parse().ok()
}

fn is_contiguous_mask(mask: Ipv4Addr) -> bool {
    let bits = u32::from(mask);
    let prefix = bits.leading_ones();
    let expected = if prefix == 0 {
        0
    } else {
        u32::MAX << (32 - prefix)
    };
    bits == expected
}

/// 二进制回应中除类型、VIP、掩码外的字段是否至少有一个非零
fn has_structural_data(payload: &[u8]) -> bool {
    payload[4..BINARY_VIP_OFFSET]
        .chunks_exact(4)
        .chain(payload[BINARY_MASK_OFFSET + 4..BINARY_MIN_LEN].chunks_exact(4))
        .any(|field| field.iter().any(|byte| *byte != 0))
}

fn trim_ascii_start(mut bytes: &[u8]) -> &[u8] {
    while let Some((first, rest)) = bytes.split_first() {
        if first.is_ascii_whitespace() {
            bytes = rest;
        } else {
            break;
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(text: &str) -> ParsedHandshake {
        parse_response(text.as_bytes())
    }

    #[test]
    fn 解析_json_成功回应() {
        let parsed = json(
            r#"{"req_packet":1,"resp_packet":{"vip_v4":"198.18.0.10","vip_mask":"255.255.248.0","dns1_v4":"198.51.100.5","dns2_v4":"198.51.100.6"}}"#,
        );
        let ParsedHandshake::Ok(info) = parsed else {
            panic!("应解析为成功回应");
        };
        assert_eq!(info.vip, Some("198.18.0.10".parse().unwrap()));
        assert_eq!(info.mask, Some("255.255.248.0".parse().unwrap()));
        assert_eq!(info.dns1, Some("198.51.100.5".parse().unwrap()));
        assert_eq!(info.dns2, Some("198.51.100.6".parse().unwrap()));
        assert_eq!(info.req_packet, Some(1));
    }

    #[test]
    fn 解析_json_错误通告() {
        let parsed = json(r#"{"req_packet":57}"#);
        let ParsedHandshake::GatewayError { code, message } = parsed else {
            panic!("应识别为错误通告");
        };
        assert_eq!(code, 57);
        assert!(message.contains("登录账号受限"));
        assert!(message.contains("错误码 57"));
    }

    #[test]
    fn 字符串错误码同样识别() {
        let parsed = json(r#"{"req_packet":"50"}"#);
        assert!(matches!(
            parsed,
            ParsedHandshake::GatewayError { code: 50, .. }
        ));
    }

    #[test]
    fn 零地址按未分配处理() {
        let parsed = json(r#"{"resp_packet":{"vip_v4":"0.0.0.0","vip_mask":"255.255.248.0"}}"#);
        assert!(matches!(
            parsed,
            ParsedHandshake::GatewayError { code: 45, .. }
        ));
    }

    #[test]
    fn 解析二进制成功回应() {
        let mut payload = vec![0u8; BINARY_MIN_LEN];
        payload[0..4].copy_from_slice(&1u32.to_be_bytes());
        payload[BINARY_VIP_OFFSET..BINARY_VIP_OFFSET + 4]
            .copy_from_slice(&u32::from("198.18.0.20".parse::<Ipv4Addr>().unwrap()).to_be_bytes());
        payload[BINARY_MASK_OFFSET..BINARY_MASK_OFFSET + 4].copy_from_slice(
            &u32::from("255.255.248.0".parse::<Ipv4Addr>().unwrap()).to_be_bytes(),
        );
        let parsed = parse_response(&payload);
        let ParsedHandshake::Ok(info) = parsed else {
            panic!("应解析为成功回应");
        };
        assert_eq!(info.vip, Some("198.18.0.20".parse().unwrap()));
        assert_eq!(info.mask, Some("255.255.248.0".parse().unwrap()));
    }

    #[test]
    fn 二进制错误码优先于地址字段() {
        let mut payload = vec![0u8; BINARY_MIN_LEN];
        payload[0..4].copy_from_slice(&50u32.to_be_bytes());
        assert!(matches!(
            parse_response(&payload),
            ParsedHandshake::GatewayError { code: 50, .. }
        ));
    }

    #[test]
    fn 结构化零地址按未分配处理() {
        let mut payload = vec![0u8; BINARY_MIN_LEN];
        payload[0..4].copy_from_slice(&1u32.to_be_bytes());
        payload[0x18] = 1;
        assert!(matches!(
            parse_response(&payload),
            ParsedHandshake::GatewayError { code: 45, .. }
        ));
    }

    #[test]
    fn 全零确认帧不按地址错误处理() {
        let mut payload = vec![0u8; BINARY_MIN_LEN];
        payload[0..4].copy_from_slice(&1u32.to_be_bytes());
        assert_eq!(parse_response(&payload), ParsedHandshake::Unknown);
    }

    #[test]
    fn 未知错误码不重复前缀() {
        assert_eq!(rejection_message(44), "网关拒绝建立隧道（错误码 44）");
    }

    #[test]
    fn 长文本不会被按二进制解析() {
        let payload = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\nxxxxxxxxxxxxxxxx";
        assert_eq!(parse_response(payload), ParsedHandshake::Unknown);
    }

    #[test]
    fn 无法识别的载荷保持_unknown() {
        assert_eq!(parse_response(b"OK"), ParsedHandshake::Unknown);
        assert_eq!(parse_response(&[0u8; 8]), ParsedHandshake::Unknown);
        assert_eq!(
            parse_response(br#"{"foo":"bar"}"#),
            ParsedHandshake::Unknown
        );
    }

    #[test]
    fn 普通帧不会被误判为错误() {
        assert_eq!(classify_error(b"OK"), None);
        assert_eq!(classify_error(&[0u8; 64]), None);
        assert!(classify_error(&57u32.to_be_bytes()).is_some());
    }

    #[test]
    fn 会话失效错误码归类() {
        for code in [39, 42, 50, 52, 57] {
            assert!(is_session_expired(code), "{code} 应判定为会话失效");
        }
        for code in [40, 41, 45, 51, 53, 56] {
            assert!(!is_session_expired(code), "{code} 不应判定为会话失效");
        }
    }

    #[test]
    fn 错误通告携带错误码与会话失效标记() {
        let error = classify_error(&39u32.to_be_bytes()).expect("应识别错误通告");
        assert_eq!(error.code, 39);
        assert!(error.session_expired());
        assert!(error.message.contains("会话 ID 无效"));

        let error = classify_error(br#"{"req_packet":53}"#).expect("应识别错误通告");
        assert_eq!(error.code, 53);
        assert!(!error.session_expired());
    }
}
