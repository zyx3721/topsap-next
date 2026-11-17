/** 连接门禁：启动预检中、正在修改 DNS，或存在未跳过的手动 DNS 时阻止连接 */
export function isDnsGateActive(input: {
  startupChecking: boolean;
  applying: boolean;
  hasManualDns: boolean;
  skipped: boolean;
}): boolean {
  return (
    input.startupChecking ||
    input.applying ||
    (input.hasManualDns && !input.skipped)
  );
}
