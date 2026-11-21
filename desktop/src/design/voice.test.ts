import { assignVoices, hash, VOICES } from "./voice";

describe("voices", () => {
  it("hash is stable and spreads", () => {
    expect(hash("abc")).toBe(hash("abc"));
    expect(hash("abc")).not.toBe(hash("abd"));
  });

  it("gives six devices six different voices, in a stable way", () => {
    const ids = ["A", "B", "C", "D", "E", "F"].map((id) => ({ id }));
    const a = assignVoices(ids);
    expect(new Set(Object.values(a)).size).toBe(6);
    expect(assignVoices(ids)).toEqual(a);
  });

  it("earlier devices keep their voice when more are added", () => {
    const first = assignVoices([{ id: "phone" }, { id: "tablet" }]);
    const later = assignVoices([{ id: "phone" }, { id: "tablet" }, { id: "laptop" }]);
    expect(later.phone).toBe(first.phone);
    expect(later.tablet).toBe(first.tablet);
  });

  it("wraps around after six", () => {
    const ids = Array.from({ length: 9 }, (_, i) => ({ id: `d${i}` }));
    const a = assignVoices(ids);
    expect(Object.keys(a)).toHaveLength(9);
    for (const v of Object.values(a)) expect(VOICES).toContain(v);
  });
});
