import { describe, expect, it } from "vitest";
import { formatRelativeTime } from "./localization";

function iso(date: Date): string {
  return date.toISOString();
}

describe("formatRelativeTime (en)", () => {
  it("returns the translated 'just now' for timestamps within the last minute", () => {
    const now = new Date();
    expect(formatRelativeTime("en", iso(now))).toBe("just now");
  });

  it("returns minutes for timestamps within the last hour", () => {
    const thirtyMinAgo = new Date(Date.now() - 30 * 60_000);
    expect(formatRelativeTime("en", iso(thirtyMinAgo))).toBe("30 minutes ago");
  });

  it("returns hours for timestamps within the last day", () => {
    const twoHoursAgo = new Date(Date.now() - 2 * 3_600_000);
    expect(formatRelativeTime("en", iso(twoHoursAgo))).toBe("2 hours ago");
  });

  it("returns days for timestamps within the last month", () => {
    const threeDaysAgo = new Date(Date.now() - 3 * 86_400_000);
    expect(formatRelativeTime("en", iso(threeDaysAgo))).toBe("3 days ago");
  });

  it("returns weeks for timestamps within the last few months", () => {
    const twoWeeksAgo = new Date(Date.now() - 14 * 86_400_000);
    expect(formatRelativeTime("en", iso(twoWeeksAgo))).toBe("2 weeks ago");
  });

  it("returns months for timestamps older than ~30 days", () => {
    const sixtyDaysAgo = new Date(Date.now() - 60 * 86_400_000);
    expect(formatRelativeTime("en", iso(sixtyDaysAgo))).toBe("2 months ago");
  });

  it("returns '1 hour ago' not '1 hours ago'", () => {
    const oneHourAgo = new Date(Date.now() - 3_600_000);
    expect(formatRelativeTime("en", iso(oneHourAgo))).toBe("1 hour ago");
  });

  it("returns '1 day ago' for yesterday", () => {
    const oneDayAgo = new Date(Date.now() - 86_400_000);
    expect(formatRelativeTime("en", iso(oneDayAgo))).toBe("1 day ago");
  });
});
