//! Windows 默认路由网卡的 DNS 状态读取与自动获取。

use std::net::IpAddr;
use std::time::Duration;

use windows_registry::LOCAL_MACHINE;

use super::DnsTarget;
use crate::netcfg::shell;
use crate::netcfg::windows::{adapter_identity_by_index, default_route_candidates};

const TCPIP_INTERFACES: &str = r"SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces";
const TCPIP6_INTERFACES: &str = r"SYSTEM\CurrentControlSet\Services\Tcpip6\Parameters\Interfaces";
const READBACK_ATTEMPTS: usize = 10;
const READBACK_INTERVAL: Duration = Duration::from_millis(200);

struct InterfaceDns {
    dns: Vec<String>,
    dhcp_enabled: Option<bool>,
}

/// 读取默认路由网卡的 DNS；手动配置返回目标，自动获取返回 `None`
pub(super) async fn inspect_default_route() -> Result<Option<DnsTarget>, String> {
    let candidates =
        default_route_candidates().map_err(|err| format!("读取路由表失败（错误码 {err}）"))?;
    let index = choose_default(&candidates)
        .ok_or_else(|| "未找到默认路由，无法修改本机 DNS".to_string())?;
    let identity = adapter_identity_by_index(index)
        .ok_or_else(|| format!("未找到接口索引 {index} 对应的网卡"))?;
    if is_virtual_adapter(&identity.friendly_name, &identity.description) {
        return Err("默认网络位于虚拟网卡，已跳过本机 DNS 修改".to_string());
    }

    let ipv4 = read_interface_dns(TCPIP_INTERFACES, &identity.guid);
    let ipv6 = read_interface_dns(TCPIP6_INTERFACES, &identity.guid);
    let mut current = ipv4.dns;
    current.extend(ipv6.dns);
    dedup(&mut current);
    if current.is_empty() {
        return Ok(None);
    }

    Ok(Some(DnsTarget {
        id: identity.guid,
        name: identity.friendly_name,
        current,
        dhcp_enabled: ipv4.dhcp_enabled,
    }))
}

/// 通过 netsh 把默认网卡的 IPv4/IPv6 DNS 改为 DHCP，并回读注册表确认
pub(super) async fn set_automatic(target: &DnsTarget) -> Result<(), String> {
    let name_arg = format!("name={}", target.name);
    let ipv4 = shell::run(
        "netsh",
        &[
            "interface",
            "ipv4",
            "set",
            "dnsservers",
            name_arg.as_str(),
            "source=dhcp",
        ],
    )
    .await;
    if !ipv4.ok {
        return Err(format!("设置 IPv4 DNS 失败：{}", ipv4.text().trim()));
    }

    let ipv6_had_static = !read_interface_dns(TCPIP6_INTERFACES, &target.id)
        .dns
        .is_empty();
    let ipv6 = shell::run(
        "netsh",
        &[
            "interface",
            "ipv6",
            "set",
            "dnsservers",
            name_arg.as_str(),
            "source=dhcp",
        ],
    )
    .await;
    if !ipv6.ok && ipv6_had_static {
        return Err(format!("设置 IPv6 DNS 失败：{}", ipv6.text().trim()));
    }

    for _ in 0..READBACK_ATTEMPTS {
        tokio::time::sleep(READBACK_INTERVAL).await;
        let ipv4 = read_interface_dns(TCPIP_INTERFACES, &target.id);
        let ipv6 = read_interface_dns(TCPIP6_INTERFACES, &target.id);
        if ipv4.dns.is_empty() && ipv6.dns.is_empty() {
            let _ = shell::run("ipconfig", &["/flushdns"]).await;
            return Ok(());
        }
    }

    Err("DNS 修改后回读仍为手动配置".to_string())
}

fn choose_default(candidates: &[(u32, u32, u32)]) -> Option<u32> {
    candidates
        .iter()
        .filter(|(_, next_hop, _)| *next_hop != 0)
        .min_by_key(|(_, _, metric)| *metric)
        .map(|(index, _, _)| *index)
}

fn read_interface_dns(root: &str, guid: &str) -> InterfaceDns {
    let path = format!(r"{root}\{guid}");
    let Ok(key) = LOCAL_MACHINE.open(&path) else {
        return InterfaceDns {
            dns: Vec::new(),
            dhcp_enabled: None,
        };
    };

    let mut dns = Vec::new();
    if let Ok(value) = key.get_string("NameServer") {
        dns.extend(parse_dns_list(&value));
    }
    if let Ok(values) = key.get_multi_string("NameServerList") {
        for value in values {
            dns.extend(parse_dns_list(&value));
        }
    }
    let dhcp_enabled = key.get_u32("EnableDHCP").ok().map(|value| value != 0);
    InterfaceDns { dns, dhcp_enabled }
}

fn parse_dns_list(value: &str) -> Vec<String> {
    value
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter_map(|part| {
            let part = part.split('%').next()?.trim();
            if part.is_empty() {
                return None;
            }
            part.parse::<IpAddr>().ok().map(|ip| ip.to_string())
        })
        .collect()
}

fn dedup(values: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

fn is_virtual_adapter(friendly_name: &str, description: &str) -> bool {
    let text = format!("{friendly_name} {description}").to_ascii_lowercase();
    [
        "topsap",
        "wintun",
        "tap-",
        "wireguard",
        "openvpn",
        "sangfor",
        "vpn",
    ]
    .iter()
    .any(|marker| text.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 解析逗号与空白分隔的_dns() {
        assert_eq!(
            parse_dns_list("192.0.2.53, 198.51.100.5  203.0.113.9"),
            vec!["192.0.2.53", "198.51.100.5", "203.0.113.9"]
        );
    }

    #[test]
    fn 解析_ipv6_并去掉作用域后缀() {
        assert_eq!(
            parse_dns_list("2001:db8::1 fe80::1%12"),
            vec!["2001:db8::1", "fe80::1"]
        );
    }

    #[test]
    fn 忽略空值与非法值() {
        assert!(parse_dns_list(" ,  , not-an-ip ").is_empty());
    }

    #[test]
    fn 选择跃点数最小的默认路由() {
        let candidates = [(12, 0x0100_000A, 20), (12, 0x0100_000A, 5), (99, 0, 1)];
        assert_eq!(choose_default(&candidates), Some(12));
    }

    #[test]
    fn 只有_on_link_默认路由时返回_none() {
        assert_eq!(choose_default(&[(12, 0, 1)]), None);
    }

    #[test]
    fn 识别已知虚拟网卡() {
        assert!(is_virtual_adapter("TopSAP", "Wintun Userspace Tunnel"));
        assert!(is_virtual_adapter(
            "Ethernet 2",
            "Sangfor SSL VPN CS Support System VNIC"
        ));
        assert!(!is_virtual_adapter(
            "以太网",
            "Intel(R) Ethernet Connection I219-LM"
        ));
    }
}
