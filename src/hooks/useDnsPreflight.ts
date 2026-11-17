import { useCallback, useEffect, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { toast } from "sonner";

import { api } from "../lib/api";
import { firstLine } from "../lib/format";
import { useI18n } from "../lib/i18n";
import { queryKeys } from "../lib/query";
import type { DnsTarget } from "../types/engine";

export interface DnsPreflightController {
  target: DnsTarget | null;
  checking: boolean;
  startupChecking: boolean;
  applying: boolean;
  skipped: boolean;
  error: string | null;
  apply: () => Promise<void>;
  refresh: () => Promise<void>;
  skip: () => void;
}

/** 启动时检查默认网络的 DNS，并维护告警卡片与按钮门禁状态 */
export function useDnsPreflight(): DnsPreflightController {
  const { t } = useI18n();
  const [target, setTarget] = useState<DnsTarget | null>(null);
  const [checking, setChecking] = useState(true);
  const [startupChecking, setStartupChecking] = useState(true);
  const [applying, setApplying] = useState(false);
  const [skipped, setSkipped] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const settingsQuery = useQuery({
    queryKey: queryKeys.settings,
    queryFn: api.loadSettings,
  });
  const settingsReady =
    settingsQuery.data !== undefined || settingsQuery.isError;
  const enabled = settingsQuery.data?.dns_preflight_on_start !== false;
  const previousEnabled = useRef<boolean | null>(null);

  const runCheck = useCallback(async (force: boolean) => {
    setChecking(true);
    setSkipped(false);
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
    if (!settingsReady) return;
    const previous = previousEnabled.current;
    previousEnabled.current = enabled;

    if (!enabled) {
      setTarget(null);
      setSkipped(false);
      setError(null);
      setChecking(false);
      setStartupChecking(false);
      return;
    }

    // 只在本次启动首次读取设置时检查；运行中重新开启等待下次启动
    if (previous === null) {
      void runCheck(false);
    }
  }, [enabled, settingsReady, runCheck]);

  const refresh = useCallback(async () => {
    if (!enabled) {
      setTarget(null);
      setSkipped(false);
      setError(null);
      return;
    }
    await runCheck(true);
  }, [enabled, runCheck]);

  const skip = useCallback(() => {
    setSkipped(true);
    toast.info(t("已跳过本机 DNS 修改，将使用当前手动 DNS 连接"));
  }, [t]);

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

  return {
    target,
    checking,
    startupChecking,
    applying,
    skipped,
    error,
    apply,
    refresh,
    skip,
  };
}
