// Locale-aware formatting for apps/web, folded in from the former relative-time.ts.
//
// Pure: the locale is an explicit parameter, so there is no global mutable locale
// singleton (the impure read of the active locale happens at the call site, via
// Paraglide's getLocale()). Translation itself stays with Paraglide's compiled m.*
// functions, imported directly where the compiler enforces key existence; this
// module owns only the locale-parameterized Intl layer Paraglide does not cover,
// and is the home for future formatters (date/number/list) as they gain callers.
import { relativeJustNow } from "../paraglide/messages.js";
import type { Locale } from "../paraglide/runtime.js";

const MINUTE = 60;
const HOUR = 3_600;
const DAY = 86_400;
const WEEK = 604_800;
const MONTH = 2_592_000;

// Two changes from the original relative-time.ts: the formatter is built from the
// active locale (was a hardcoded "en" singleton), and the sub-minute branch returns
// the translated relativeJustNow() instead of a baked-in English literal. Building
// Intl.RelativeTimeFormat per call is negligible at our volume (a handful of cards);
// memoize per locale only if that changes.
export function formatRelativeTime(locale: Locale, isoTimestamp: string): string {
  const seconds = Math.round((Date.now() - new Date(isoTimestamp).getTime()) / 1_000);

  if (seconds < MINUTE) return relativeJustNow();

  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "always" });
  if (seconds < HOUR) return rtf.format(-Math.round(seconds / MINUTE), "minute");
  if (seconds < DAY) return rtf.format(-Math.round(seconds / HOUR), "hour");
  if (seconds < WEEK) return rtf.format(-Math.round(seconds / DAY), "day");
  if (seconds < MONTH) return rtf.format(-Math.round(seconds / WEEK), "week");
  return rtf.format(-Math.round(seconds / MONTH), "month");
}
