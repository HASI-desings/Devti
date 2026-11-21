import { render } from "@testing-library/react";
import { ALL_ICONS, deviceIcon, Icon } from "./icons";

describe("icons", () => {
  it("every icon renders paths on the 24-unit grid", () => {
    for (const name of ALL_ICONS) {
      const { container, unmount } = render(<Icon name={name} />);
      const svg = container.querySelector("svg")!;
      expect(svg.getAttribute("viewBox")).toBe("0 0 24 24");
      expect(svg.querySelectorAll("path, circle").length).toBeGreaterThan(0);
      unmount();
    }
  });

  it("device icons carry the signature dot, tinted on request", () => {
    const { container } = render(<Icon name={deviceIcon("phone")} dotColor="red" title="Phone" />);
    expect(container.querySelector("circle")?.getAttribute("fill")).toBe("red");
    expect(container.querySelector("svg")?.getAttribute("aria-label")).toBe("Phone");
    expect(deviceIcon("unknown")).toBe("monitor");
  });
});
