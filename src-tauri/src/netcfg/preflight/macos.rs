//! macOS 默认网络服务的 DNS 状态读取与自动获取。

use std::time::Duration;

use super::DnsTarget;
use crate::netcfg::macos_text::{parse_dns_mode, DnsMode};
use crate::netcfg::shell;
use crate::netcfg::unix::{default_interface_name, service_for_interface};

const READBACK_ATTEMPTS: usize = 10;
const READBACK_INTERVAL: Duration = Duration::from_millis(200);

/// 读取默认网络服务的 DNS；手动配置返回目标，自动获取返回 `None`
pub(super) async fn inspect_default_route() -> Result<Option<DnsTarget>, String> {
    let ifname = default_interface_name()
        .await
        .ok_or_else(|| "未找到默认路由，无法修改本机 DNS".to_string())?;
    if is_virtual_interface(&ifname) {
        return Err("默认网络位于虚拟网卡，已跳过本机 DNS 修改".to_string());
    }
    let service = service_for_interface(&ifname)
        .await
        .ok_or_else(|| format!("未找到默认接口 {ifname} 对应的网络服务"))?;
    let text = read_dns(&service).await?;
    match parse_dns_mode(&text) {
        DnsMode::Automatic => Ok(None),
        DnsMode::Static(current) => Ok(Some(DnsTarget {
            id: service.clone(),
            name: service,
            current,
            dhcp_enabled: None,
        })),
        DnsMode::Unknown => Err("无法识别网络服务的 DNS 配置".to_string()),
    }
}

/// 通过 networksetup 把网络服务 DNS 置空，并回读确认
pub(super) async fn set_automatic(target: &DnsTarget) -> Result<(), String> {
    let out = shell::run(
        "networksetup",
        &["setdnsservers", target.id.as_str(), "Empty"],
    )
    .await;
    if !out.ok {
        return Err(format!("设置 DNS 失败：{}", out.text().trim()));
    }

    for _ in 0..READBACK_ATTEMPTS {
        tokio::time::sleep(READBACK_INTERVAL).await;
        if let Ok(text) = read_dns(&target.id).await {
            if matches!(parse_dns_mode(&text), DnsMode::Automatic) {
                return Ok(());
            }
        }
    }

    Err("DNS 修改后回读仍为手动配置".to_string())
}

async fn read_dns(service: &str) -> Result<String, String> {
    let out = shell::run_local("networksetup", &["-getdnsservers", service]).await;
    if out.ok {
        return Ok(out.stdout);
    }
    let out = shell::run("networksetup", &["-getdnsservers", service]).await;
    if out.ok {
        Ok(out.stdout)
    } else {
        Err(format!("读取 DNS 失败：{}", out.text().trim()))
    }
}

fn is_virtual_interface(ifname: &str) -> bool {
    ifname.starts_with("utun")
        || ifname.starts_with("tun")
        || ifname.starts_with("ppp")
        || ifname.starts_with("ipsec")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 识别虚拟接口() {
        assert!(is_virtual_interface("utun8"));
        assert!(is_virtual_interface("ppp0"));
        assert!(is_virtual_interface("ipsec0"));
        assert!(!is_virtual_interface("en0"));
    }
}
