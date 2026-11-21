import "@testing-library/jest-dom/vitest";

// jsdom lacks these browser APIs the UI relies on.
if (!window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }) as unknown as MediaQueryList;
}
class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
// biome-ignore lint/suspicious/noExplicitAny: test shim
(globalThis as any).ResizeObserver ??= RO;
HTMLCanvasElement.prototype.getContext = (() => null) as never;
