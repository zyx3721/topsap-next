import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";

import { api } from "../lib/api";
import { firstLine } from "../lib/format";
import { useI18n } from "../lib/i18n";
import type { DnsTarget } from "../types/engine";

export interface DnsPreflightController {
  target: DnsTarget | null;
  checking: boolean;
  startupChecking: boolean;
  applying: boolean;
  error: string | null;
  apply: () => Promise<void>;
  refresh: () => Promise<void>;
}

/** 启动时检查默认网络的 DNS，并维护告警卡片与按钮门禁状态 */
export function useDnsPreflight(): DnsPreflightController {
  const { t } = useI18n();
  const [target, setTarget] = useState<DnsTarget | null>(null);
  const [checking, setChecking] = useState(true);
  const [startupChecking, setStartupChecking] = useState(true);
  const [applying, setApplying] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const runCheck = useCallback(async (force: boolean) => {
    setChecking(true);
    setError(null);
    try {
      const result = await api.inspectNetworkDns(force);
      setTarget(result.target);
      if (result.error && force) {
        toast.error(firstLine(result.error));
      }
    } catch (err) {
      const message = firstLine(String(err));
      if (force) toast.error(message);
      setError(message);
    } finally {
      setChecking(false);
      setStartupChecking(false);
    }
  }, []);

  useEffect(() => {
    void runCheck(false);
  }, [runCheck]);

  const refresh = useCallback(async () => {
    await runCheck(true);
  }, [runCheck]);

  const apply = useCallback(async () => {
    if (applying) return;
    setApplying(true);
    setError(null);
    try {
      await api.setNetworkDnsAuto();
      setTarget(null);
      toast.success(t("已将本机 DNS 改为自动获取"));
    } catch (err) {
      const message = firstLine(String(err));
      setError(message);
      toast.error(message);
    } finally {
      setApplying(false);
    }
  }, [applying, t]);

  return { target, checking, startupChecking, applying, error, apply, refresh };
}
