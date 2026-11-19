import type { ResourceEntry } from "../types/engine";

const STORAGE_KEY = "topsap.pinnedResources";

type PinStorage = Pick<Storage, "getItem" | "setItem">;

function defaultStorage(): PinStorage | null {
  if (typeof window === "undefined") return null;
  return window.localStorage;
}

/** 置顶标识按分组、名称与地址区分，列表刷新后仍能匹配同一资源 */
export function resourcePinKey(item: ResourceEntry): string {
  return `${item.group}\u0000${item.name}\u0000${item.url}`;
}

/** 读取置顶标识；数据缺失或损坏时回退为空数组 */
export function readPinnedKeys(storage: PinStorage | null = defaultStorage()): string[] {
  if (!storage) return [];
  try {
    const raw = storage.getItem(STORAGE_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return Array.from(
      new Set(
        parsed.filter(
          (item): item is string => typeof item === "string" && item.length > 0,
        ),
      ),
    );
  } catch {
    return [];
  }
}

/** 保存置顶标识；存储不可用时返回 false，本次会话内仍保留置顶状态 */
export function writePinnedKeys(
  keys: string[],
  storage: PinStorage | null = defaultStorage(),
): boolean {
  if (!storage) return false;
  try {
    storage.setItem(STORAGE_KEY, JSON.stringify(keys));
    return true;
  } catch {
    return false;
  }
}
