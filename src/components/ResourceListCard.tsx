import { useMemo, useState } from "react";
import { ChevronDown, ChevronUp, Globe2, Pin, PinOff, Search } from "lucide-react";
import { toast } from "sonner";

import { api } from "../lib/api";
import { firstLine } from "../lib/format";
import { useI18n } from "../lib/i18n";
import {
  readPinnedKeys,
  resourcePinKey,
  writePinnedKeys,
} from "../lib/resourcePins";
import type { ResourceEntry } from "../types/engine";
import { Badge, Card, CardHeader, EmptyState, Input, cn } from "./ui";

const isDefaultGroup = (group: string) =>
  group === "default" || group === "defaultGroup";

const canOpenResource = (item: ResourceEntry) =>
  /^(https?|smb):\/\//i.test(item.url.trim());

function ResourceTile({
  item,
  pinned,
  onTogglePin,
  onOpen,
}: {
  item: ResourceEntry;
  pinned: boolean;
  onTogglePin: (key: string) => void;
  onOpen: (item: ResourceEntry) => void;
}) {
  const { t } = useI18n();
  const openable = canOpenResource(item);
  const key = resourcePinKey(item);
  return (
    <div className="group relative">
      <div
        role={openable ? "button" : undefined}
        tabIndex={openable ? 0 : undefined}
        title={openable ? t("打开资源") : undefined}
        onClick={openable ? () => onOpen(item) : undefined}
        onKeyDown={(event) => {
          if (!openable) return;
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            onOpen(item);
          }
        }}
        className={cn(
          "flex items-center gap-2.5 rounded-xl border border-border bg-surface-2/40 py-2 pl-3 pr-9",
          "transition group-hover:border-accent/40 group-hover:bg-hover",
          openable ? "cursor-pointer" : "cursor-default",
        )}
      >
        <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-accent-soft text-accent">
          <Globe2 className="h-4 w-4" />
        </span>
        <span className="min-w-0">
          <span className="block truncate text-xs font-medium text-text">
            {item.name}
          </span>
          {item.url ? (
            <span className="block truncate font-mono text-[10px] text-faint">
              {item.url}
            </span>
          ) : null}
        </span>
      </div>
      <button
        type="button"
        title={pinned ? t("取消置顶") : t("置顶")}
        aria-label={pinned ? t("取消置顶") : t("置顶")}
        aria-pressed={pinned}
        onClick={() => onTogglePin(key)}
        className={cn(
          "absolute right-1 top-0.5 flex h-6 w-6 items-center justify-center rounded-md transition",
          "hover:bg-hover focus-visible:opacity-100",
          pinned
            ? "text-accent opacity-0 group-hover:opacity-100"
            : "text-muted opacity-0 hover:text-accent group-hover:opacity-100",
        )}
      >
        {pinned ? (
          <PinOff className="h-3.5 w-3.5" />
        ) : (
          <Pin className="h-3.5 w-3.5" />
        )}
      </button>
    </div>
  );
}

export function ResourceListCard({ resources }: { resources: ResourceEntry[] }) {
  const { t, lang } = useI18n();
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [pinnedKeys, setPinnedKeys] = useState<string[]>(readPinnedKeys);

  const { groups, pinnedItems } = useMemo(() => {
    const keyword = query.trim().toLowerCase();
    const matches = (item: ResourceEntry) =>
      !keyword ||
      `${item.name} ${item.url} ${item.group}`.toLowerCase().includes(keyword);
    const filtered = resources.filter(matches);
    const map = new Map<string, ResourceEntry[]>();
    for (const item of filtered) {
      const group = item.group || "default";
      map.set(group, [...(map.get(group) ?? []), item]);
    }
    const groupEntries = Array.from(map.entries())
      .map(([group, items]) => ({
        group,
        label: isDefaultGroup(group) ? t("默认组") : group,
        items: items.slice().sort((a, b) => a.name.localeCompare(b.name)),
      }))
      .sort((a, b) => {
        const aDefault = isDefaultGroup(a.group);
        const bDefault = isDefaultGroup(b.group);
        if (aDefault !== bDefault) return aDefault ? -1 : 1;
        return a.group.localeCompare(b.group, "zh-CN");
      });

    const byKey = new Map<string, ResourceEntry>();
    for (const item of resources) byKey.set(resourcePinKey(item), item);
    const pinnedEntries = pinnedKeys
      .map((key) => byKey.get(key))
      .filter((item): item is ResourceEntry => item !== undefined && matches(item));

    return { groups: groupEntries, pinnedItems: pinnedEntries };
  }, [resources, query, t, pinnedKeys]);

  const toggle = (group: string) => {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(group)) next.delete(group);
      else next.add(group);
      return next;
    });
  };

  const togglePin = (key: string) => {
    const next = pinnedKeys.includes(key)
      ? pinnedKeys.filter((item) => item !== key)
      : [key, ...pinnedKeys];
    setPinnedKeys(next);
    writePinnedKeys(next);
  };

  const openResource = async (item: ResourceEntry) => {
    if (!canOpenResource(item)) return;
    try {
      await api.openUrl(item.url.trim());
    } catch (error) {
      toast.error(firstLine(String(error)));
    }
  };

  return (
    <Card>
      <CardHeader
        title={t("资源列表")}
        actions={
          <div className="relative w-52">
            <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-faint" />
            <Input
              value={query}
              onChange={(event) => setQuery(event.target.value)}
              placeholder={t("搜索资源")}
              className="h-8 pl-8 text-xs"
            />
          </div>
        }
      />
      <div className="max-h-[420px] space-y-4 overflow-y-auto p-5">
        {pinnedItems.length > 0 ? (
          <section>
            <div className="flex items-center gap-2 py-1 text-sm font-medium text-text">
              <Pin className="h-3.5 w-3.5 text-accent" />
              {lang === "zh" ? t("置顶") : t("置顶资源")}
              <Badge tone="accent">{pinnedItems.length}</Badge>
            </div>
            <div className="mt-2 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
              {pinnedItems.map((item) => (
                <ResourceTile
                  key={`pinned-${resourcePinKey(item)}`}
                  item={item}
                  pinned
                  onTogglePin={togglePin}
                  onOpen={(entry) => void openResource(entry)}
                />
              ))}
            </div>
          </section>
        ) : null}
        {groups.length === 0 ? (
          <EmptyState
            icon={<Globe2 className="h-6 w-6" />}
            title={t("暂无资源")}
            description={t("网关未下发资源，或搜索无匹配结果")}
          />
        ) : (
          groups.map((group) => {
            const isCollapsed = collapsed.has(group.group);
            return (
              <section key={group.group}>
                <button
                  type="button"
                  onClick={() => toggle(group.group)}
                  className="flex w-full items-center justify-between gap-2 rounded-lg py-1 text-left text-sm font-medium text-text transition hover:text-accent"
                >
                  <span className="flex items-center gap-2">
                    {group.label}
                    <Badge tone="neutral">{group.items.length}</Badge>
                  </span>
                  {isCollapsed ? (
                    <ChevronDown className="h-3.5 w-3.5 text-faint" />
                  ) : (
                    <ChevronUp className="h-3.5 w-3.5 text-faint" />
                  )}
                </button>
                {isCollapsed ? null : (
                  <div className="mt-2 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
                    {group.items.map((item) => {
                      const key = resourcePinKey(item);
                      return (
                        <ResourceTile
                          key={`${group.group}-${item.name}-${item.url}`}
                          item={item}
                          pinned={pinnedKeys.includes(key)}
                          onTogglePin={togglePin}
                          onOpen={(entry) => void openResource(entry)}
                        />
                      );
                    })}
                  </div>
                )}
              </section>
            );
          })
        )}
      </div>
    </Card>
  );
}
