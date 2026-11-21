import { trackIdle } from "./idle";

describe("idle tracker", () => {
  it("rests after the quiet period and wakes on input", () => {
    vi.useFakeTimers();
    const stop = trackIdle(document, 1000);
    expect(document.documentElement.dataset.idle).toBe("false");
    vi.advanceTimersByTime(1100);
    expect(document.documentElement.dataset.idle).toBe("true");
    document.dispatchEvent(new Event("keydown"));
    expect(document.documentElement.dataset.idle).toBe("false");
    stop();
    vi.useRealTimers();
  });
});
