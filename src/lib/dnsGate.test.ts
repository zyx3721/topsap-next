import { describe, expect, it } from "vitest";

import { isDnsGateActive } from "./dnsGate";

const base = {
  startupChecking: false,
  applying: false,
  hasManualDns: false,
  skipped: false,
};

describe("isDnsGateActive", () => {
  it("首次预检未结束时阻止连接", () => {
    expect(isDnsGateActive({ ...base, startupChecking: true })).toBe(true);
  });

  it("正在修改 DNS 时阻止连接", () => {
    expect(isDnsGateActive({ ...base, applying: true })).toBe(true);
  });

  it("存在未跳过的手动 DNS 时阻止连接", () => {
    expect(isDnsGateActive({ ...base, hasManualDns: true })).toBe(true);
  });

  it("手动 DNS 已跳过后放行", () => {
    expect(
      isDnsGateActive({ ...base, hasManualDns: true, skipped: true }),
    ).toBe(false);
  });

  it("没有手动 DNS 时放行", () => {
    expect(isDnsGateActive(base)).toBe(false);
  });
});
