//! 启动时 DNS 预检缓存：供界面门禁与启动自动连接等待同一结果。

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::OnceCell;

use crate::dto::{DnsInspectionDto, DnsTargetDto};
use crate::netcfg::preflight;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(5);

pub struct DnsPreflight {
    result: OnceCell<DnsInspectionDto>,
}

impl DnsPreflight {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            result: OnceCell::new(),
        })
    }

    /// 等待本次启动的首次预检；检测失败或超时也返回结果并释放门禁
    pub async fn ensure(&self) -> DnsInspectionDto {
        self.result.get_or_init(run_check).await.clone()
    }
}

/// 执行一次预检；所有失败都折叠为 `DnsInspectionDto.error`
pub async fn run_check() -> DnsInspectionDto {
    match tokio::time::timeout(STARTUP_TIMEOUT, preflight::inspect_default_route()).await {
        Ok(Ok(target)) => {
            let target = target.map(DnsTargetDto::from);
            if let Some(target) = &target {
                log::info!(
                    "本机 DNS 预检：{} 使用手动 DNS（{}）",
                    target.name,
                    target.current.join("、")
                );
            } else {
                log::info!("本机 DNS 预检：默认网络为自动获取");
            }
            DnsInspectionDto {
                target,
                error: None,
            }
        }
        Ok(Err(err)) => {
            log::info!("本机 DNS 预检未完成：{err}");
            DnsInspectionDto {
                target: None,
                error: Some(err),
            }
        }
        Err(_) => {
            let message = format!("本机 DNS 预检超时（{} 秒）", STARTUP_TIMEOUT.as_secs());
            log::info!("{message}");
            DnsInspectionDto {
                target: None,
                error: Some(message),
            }
        }
    }
}
