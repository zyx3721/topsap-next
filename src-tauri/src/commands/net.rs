//! 本机网络状态命令。

use tauri::State;

use crate::dto::DnsInspectionDto;
use crate::AppState;

/// 检查默认路由网卡的 DNS 是否为自动获取
#[tauri::command]
pub async fn inspect_network_dns(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    force: bool,
) -> Result<DnsInspectionDto, String> {
    if !crate::settings::load(&app).dns_preflight_on_start {
        return Ok(DnsInspectionDto {
            target: None,
            error: None,
        });
    }
    if force {
        Ok(crate::dns_preflight::run_check().await)
    } else {
        Ok(state.dns_preflight.ensure().await)
    }
}

/// 把当前默认网络改为自动获取 DNS
#[tauri::command]
pub async fn set_network_dns_auto(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.ensure_dns_change_allowed()?;
    crate::netcfg::preflight::set_current_automatic().await?;
    state
        .engine
        .report_log("已将本机 DNS 改为自动获取".to_string());
    Ok(())
}
