import { actionLabel, formatCode, kindLabel, latencyLabel, platformLabel, relativeTime } from "./format";

describe("format", () => {
  const now = 1_700_000_000_000;
  it("relative time reads naturally", () => {
    expect(relativeTime(null)).toBe("never");
    expect(relativeTime(now - 3_000, now)).toBe("just now");
    expect(relativeTime(now - 30_000, now)).toBe("30 seconds ago");
    expect(relativeTime(now - 60_000, now)).toBe("a minute ago");
    expect(relativeTime(now - 5 * 60_000, now)).toBe("5 minutes ago");
    expect(relativeTime(now - 3_600_000, now)).toBe("an hour ago");
    expect(relativeTime(now - 86_400_000, now)).toBe("yesterday");
    expect(relativeTime(now - 3 * 86_400_000, now)).toBe("3 days ago");
  });
  it("labels", () => {
    expect(kindLabel("phone")).toBe("Phone");
    expect(kindLabel("unknown")).toBe("Device");
    expect(platformLabel("ipados")).toBe("iPadOS");
    expect(latencyLabel(4)).toBe("Excellent");
    expect(latencyLabel(30)).toBe("Good");
    expect(latencyLabel(70)).toBe("Fair");
    expect(latencyLabel(300)).toBe("Slow");
    expect(actionLabel("paired")).toBe("Paired");
    expect(actionLabel("something_new")).toBe("something new");
    expect(formatCode("482913")).toBe("482 913");
    expect(formatCode("12")).toBe("12");
  });
});
