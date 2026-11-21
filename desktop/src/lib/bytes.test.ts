import { etaLabel, formatBytes } from "./bytes";

describe("bytes", () => {
  it("formats sizes", () => {
    expect(formatBytes(12)).toBe("12 B");
    expect(formatBytes(1500)).toBe("1.5 KB");
    expect(formatBytes(48_000_000)).toBe("48.0 MB");
    expect(formatBytes(250_000_000_000)).toBe("250 GB");
  });
  it("estimates time left", () => {
    expect(etaLabel(0, 10)).toBeNull();
    expect(etaLabel(10, 0)).toBeNull();
    expect(etaLabel(10, 10)).toBe("a few seconds left");
    expect(etaLabel(300, 10)).toBe("30 seconds left");
    expect(etaLabel(1200, 10)).toBe("about 2 minutes left");
  });
});
