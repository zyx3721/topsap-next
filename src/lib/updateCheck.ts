/** 运行期间定时检查更新的间隔（分钟）；已关闭或设置未就绪时返回 null */
export function updateCheckIntervalMinutes(
  settings:
    | {
        update_check_disabled: boolean;
        update_check_interval_min: number;
      }
    | undefined,
): number | null {
  if (!settings || settings.update_check_disabled) return null;
  const minutes = Math.trunc(settings.update_check_interval_min);
  if (!Number.isFinite(minutes) || minutes < 1) return 10;
  return Math.min(minutes, 1440);
}
