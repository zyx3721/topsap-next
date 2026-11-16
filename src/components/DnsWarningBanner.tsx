import { AlertTriangle, Loader2, RefreshCw } from "lucide-react";

import { useI18n } from "../lib/i18n";
import type { DnsTarget } from "../types/engine";
import { Badge, Button } from "./ui";

/** 启动预检发现手动 DNS 时展示的告警卡片 */
export function DnsWarningBanner({
  target,
  applying,
  error,
  disabled,
  disabledHint,
  autoConnectPaused,
  onApply,
  onRefresh,
}: {
  target: DnsTarget | null;
  applying: boolean;
  error: string | null;
  disabled: boolean;
  disabledHint?: string;
  autoConnectPaused?: boolean;
  onApply: () => void;
  onRefresh: () => void;
}) {
  const { t } = useI18n();
  if (!target) return null;

  return (
    <section className="rounded-2xl border border-warn/25 bg-warn-soft px-4 py-3 shadow-[var(--shadow)]">
      <div className="flex items-start gap-3">
        <AlertTriangle className="mt-0.5 h-5 w-5 shrink-0 text-warn" />
        <div className="min-w-0 flex-1 space-y-3">
          <div className="space-y-1">
            <h3 className="text-sm font-semibold text-text">
              {t("本机 DNS 不是自动获取")}
            </h3>
            <p className="text-xs leading-relaxed text-muted">
              {t(
                "检测到默认网络「{name}」使用手动配置的 DNS。手动 DNS 可能导致网关域名或内网域名解析异常。",
                { name: target.name },
              )}
            </p>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <span className="text-[11px] text-faint">{t("当前 DNS：")}</span>
            {target.current.map((server) => (
              <Badge key={server} tone="warn">
                {server}
              </Badge>
            ))}
          </div>

          <p className="text-[11px] leading-relaxed text-warn">
            {t(
              "改为自动获取会清除这些手动 DNS，且断开连接后不会恢复原设置。若当前网络要求手动 DNS，可能影响其他应用或内网访问。",
            )}
          </p>

          {target.dhcp_enabled === false ? (
            <p className="text-[11px] leading-relaxed text-warn">
              {t(
                "该网卡使用静态 IP，改为自动获取 DNS 后可能没有可用的 DNS 服务器，请确认网络支持 DHCP。",
              )}
            </p>
          ) : null}

          {autoConnectPaused ? (
            <p className="text-[11px] leading-relaxed text-warn">
              {t("启动自动连接已暂停；改为自动获取后请手动点击「连接」。")}
            </p>
          ) : null}

          {error ? (
            <p className="text-[11px] leading-relaxed text-danger">{error}</p>
          ) : null}
          {disabled && disabledHint ? (
            <p className="text-[11px] text-faint">{disabledHint}</p>
          ) : null}

          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="primary"
              size="sm"
              onClick={onApply}
              disabled={disabled || applying}
            >
              {applying ? <Loader2 className="h-4 w-4 animate-spin" /> : null}
              {applying ? t("正在修改…") : t("改为自动获取")}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              onClick={onRefresh}
              disabled={disabled || applying}
            >
              <RefreshCw className="h-3.5 w-3.5" />
              {t("重新检查")}
            </Button>
          </div>
        </div>
      </div>
    </section>
  );
}
