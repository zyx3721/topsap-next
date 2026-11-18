import { describe, expect, it } from "vitest";

import { updateCheckIntervalMinutes } from "./updateCheck";

describe("updateCheckIntervalMinutes", () => {
  it("设置未就绪或已关闭时返回 null", () => {
    expect(updateCheckIntervalMinutes(undefined)).toBeNull();
    expect(
      updateCheckIntervalMinutes({
        update_check_disabled: true,
        update_check_interval_min: 10,
      }),
    ).toBeNull();
  });

  it("默认按 10 分钟检查", () => {
    expect(
      updateCheckIntervalMinutes({
        update_check_disabled: false,
        update_check_interval_min: 10,
      }),
    ).toBe(10);
  });

  it("非法间隔回退到 10 分钟", () => {
    for (const value of [0, -5, Number.NaN]) {
      expect(
        updateCheckIntervalMinutes({
          update_check_disabled: false,
          update_check_interval_min: value,
        }),
      ).toBe(10);
    }
  });

  it("间隔上限为一天", () => {
    expect(
      updateCheckIntervalMinutes({
        update_check_disabled: false,
        update_check_interval_min: 5000,
      }),
    ).toBe(1440);
  });
});
