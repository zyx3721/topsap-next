//! 启动时本机 DNS 预检：识别默认路由网卡或网络服务的 DNS 是否为自动获取。

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// 预检目标；`id` 仅供平台实现回读使用，不返回前端
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsTarget {
    pub id: String,
    pub name: String,
    pub current: Vec<String>,
    pub dhcp_enabled: Option<bool>,
}

/// 检查默认路由的 DNS 状态；`Ok(None)` 表示自动获取
#[cfg(target_os = "windows")]
pub async fn inspect_default_route() -> Result<Option<DnsTarget>, String> {
    windows::inspect_default_route().await
}

/// 检查默认路由的 DNS 状态；`Ok(None)` 表示自动获取
#[cfg(target_os = "macos")]
pub async fn inspect_default_route() -> Result<Option<DnsTarget>, String> {
    macos::inspect_default_route().await
}

/// 非 Windows / macOS 平台不处理本机 DNS
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub async fn inspect_default_route() -> Result<Option<DnsTarget>, String> {
    Ok(None)
}

/// 把指定默认网络改为自动获取 DNS，并回读确认
#[cfg(target_os = "windows")]
pub async fn set_automatic(target: &DnsTarget) -> Result<(), String> {
    windows::set_automatic(target).await
}

/// 把指定默认网络改为自动获取 DNS，并回读确认
#[cfg(target_os = "macos")]
pub async fn set_automatic(target: &DnsTarget) -> Result<(), String> {
    macos::set_automatic(target).await
}

/// 非 Windows / macOS 平台不支持修改本机 DNS
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub async fn set_automatic(_target: &DnsTarget) -> Result<(), String> {
    Err("当前平台不支持修改本机 DNS".to_string())
}

/// 重新检测当前默认路由并改为自动获取；已是自动获取时幂等成功
pub async fn set_current_automatic() -> Result<(), String> {
    match inspect_default_route().await? {
        Some(target) => set_automatic(&target).await,
        None => Ok(()),
    }
}
