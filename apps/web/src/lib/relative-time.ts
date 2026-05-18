const MINUTE = 60;
const HOUR = 3_600;
const DAY = 86_400;
const WEEK = 604_800;
const MONTH = 2_592_000;

const rtf = new Intl.RelativeTimeFormat("en", { numeric: "always" });

export function relativeTime(isoTimestamp: string): string {
  const seconds = Math.round((Date.now() - new Date(isoTimestamp).getTime()) / 1_000);

  if (seconds < MINUTE) return "just now";
  if (seconds < HOUR) return rtf.format(-Math.round(seconds / MINUTE), "minute");
  if (seconds < DAY) return rtf.format(-Math.round(seconds / HOUR), "hour");
  if (seconds < WEEK) return rtf.format(-Math.round(seconds / DAY), "day");
  if (seconds < MONTH) return rtf.format(-Math.round(seconds / WEEK), "week");
  return rtf.format(-Math.round(seconds / MONTH), "month");
}
