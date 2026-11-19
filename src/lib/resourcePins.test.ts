import { describe, expect, it } from "vitest";

import type { ResourceEntry } from "../types/engine";
import { readPinnedKeys, resourcePinKey, writePinnedKeys } from "./resourcePins";

function makeStorage(initial?: string) {
  const values = new Map<string, string>();
  if (initial !== undefined) values.set("topsap.pinnedResources", initial);
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
}

const resource: ResourceEntry = {
  name: "10.24.1.31",
  url: "https://10.24.1.31",
  group: "数金总部",
  module: "web",
};

describe("resourcePinKey", () => {
  it("相同资源生成稳定标识", () => {
    expect(resourcePinKey(resource)).toBe(resourcePinKey({ ...resource }));
  });

  it("分组、名称或地址不同即区分", () => {
    const base = resourcePinKey(resource);
    expect(resourcePinKey({ ...resource, group: "长亮控股" })).not.toBe(base);
    expect(resourcePinKey({ ...resource, name: "10.24.1.32" })).not.toBe(base);
    expect(resourcePinKey({ ...resource, url: "https://10.24.1.32" })).not.toBe(base);
  });

  it("同名同地址资源可按分组分别置顶且互不覆盖", () => {
    const first = { ...resource, group: "数金总部" };
    const second = { ...resource, group: "长亮控股" };
    const pinned = new Map([
      [resourcePinKey(first), first],
      [resourcePinKey(second), second],
    ]);
    expect(pinned.size).toBe(2);
    expect(pinned.get(resourcePinKey(first))?.group).toBe("数金总部");
    expect(pinned.get(resourcePinKey(second))?.group).toBe("长亮控股");
  });

  it("字段拼接不会因边界重叠产生相同标识", () => {
    const first = { ...resource, group: "ab", name: "c", url: "" };
    const second = { ...resource, group: "a", name: "bc", url: "" };
    expect(resourcePinKey(first)).not.toBe(resourcePinKey(second));
  });
});

describe("readPinnedKeys", () => {
  it("无存储或未写入时返回空数组", () => {
    expect(readPinnedKeys(null)).toEqual([]);
    expect(readPinnedKeys(makeStorage())).toEqual([]);
  });

  it("读取字符串数组并去重", () => {
    const storage = makeStorage(JSON.stringify(["a", "b", "a"]));
    expect(readPinnedKeys(storage)).toEqual(["a", "b"]);
  });

  it("过滤非字符串与空字符串", () => {
    const storage = makeStorage(JSON.stringify(["a", 1, null, "", "b"]));
    expect(readPinnedKeys(storage)).toEqual(["a", "b"]);
  });

  it("非法 JSON 或非数组回退为空数组", () => {
    expect(readPinnedKeys(makeStorage("{bad"))).toEqual([]);
    expect(readPinnedKeys(makeStorage(JSON.stringify({ a: 1 })))).toEqual([]);
  });
});

describe("writePinnedKeys", () => {
  it("写入后可原样读回", () => {
    const storage = makeStorage();
    expect(writePinnedKeys(["a", "b"], storage)).toBe(true);
    expect(readPinnedKeys(storage)).toEqual(["a", "b"]);
  });

  it("存储不可用或写入抛错时返回 false", () => {
    expect(writePinnedKeys(["a"], null)).toBe(false);
    const storage = {
      getItem: () => null,
      setItem: () => {
        throw new Error("quota exceeded");
      },
    };
    expect(writePinnedKeys(["a"], storage)).toBe(false);
  });
});
