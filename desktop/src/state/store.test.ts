import type { DeviceInfo, LocalSnapshot, PeerSnapshot } from "../bridge";
import { orderedPeers, useStore } from "./store";

const info = (id: string, name = id): DeviceInfo => ({
  id,
  name,
  kind: "phone",
  platform: "android",
  os_version: "Android 15",
  model: null,
  app_version: "0.1.0",
  capabilities: [],
});

const peer = (id: string, online: boolean, paired_at = 1): PeerSnapshot => ({
  info: info(id),
  fingerprint: "ab",
  role: "trusted",
  permissions: {
    screen_view: true,
    control: true,
    files: true,
    clipboard: true,
    notifications: true,
    commands: true,
    bluetooth: true,
  },
  paired_at,
  last_seen: null,
  online,
  connection: null,
  status: { battery: null },
});

const local: LocalSnapshot = {
  info: info("HUB", "Hub"),
  fingerprint: "ff",
  port: 47810,
  addresses: [],
  status: { battery: null },
  paused: false,
  discovery_enabled: true,
  identity_origin: "created",
  secret_backend: "test",
  secret_os_protected: true,
};

beforeEach(() => {
  useStore.getState().load({ local, peers: [], nearby: [], window: null, settings: {} }, "preview");
  useStore.setState({
    pairing: null,
    toasts: [],
    justLinked: {},
    selected: null,
    page: "ecosystem",
    deviceId: null,
  });
});

describe("store", () => {
  it("tracks devices coming online for the linking animation", () => {
    const st = useStore.getState();
    st.apply({ type: "peer_changed", peer: peer("A", false) });
    expect(useStore.getState().justLinked.A).toBeUndefined();
    st.apply({ type: "peer_changed", peer: peer("A", true) });
    expect(useStore.getState().justLinked.A).toBeGreaterThan(0);
    st.apply({ type: "peer_changed", peer: peer("A", false) });
    expect(useStore.getState().justLinked.A).toBeUndefined();
    expect(useStore.getState().voices.A).toBeDefined();
  });

  it("walks the pairing stages", () => {
    const st = useStore.getState();
    st.apply({
      type: "pairing_window_changed",
      window: { id: 1, uri: "devti://x", code: "123 456", expires_at: 0, addresses: [] },
    });
    expect(useStore.getState().pairing).toEqual({ stage: "waiting" });
    st.apply({ type: "pairing_started", pairing_id: 9, role: "hub", peer: info("P") });
    expect(useStore.getState().pairing?.stage).toBe("connecting");
    st.apply({ type: "pairing_verify", pairing_id: 9, role: "hub", peer: info("P"), sas: "111 222" });
    const v = useStore.getState().pairing;
    expect(v?.stage === "verify" && v.sas).toBe("111 222");
    st.apply({ type: "pairing_failed", pairing_id: 9, reason: "rejected", message: "no" });
    expect(useStore.getState().pairing?.stage).toBe("failed");
    st.apply({ type: "pairing_completed", pairing_id: 9, peer: peer("P", true) });
    expect(useStore.getState().pairing?.stage).toBe("done");
  });

  it("removing a device clears selection and leaves its page", () => {
    const st = useStore.getState();
    st.apply({ type: "peer_changed", peer: peer("A", true) });
    st.select("A");
    st.navigate("device", "A");
    st.apply({ type: "peer_removed", id: "A", reason: "revoked_by_peer" });
    const s = useStore.getState();
    expect(s.peers.A).toBeUndefined();
    expect(s.selected).toBeNull();
    expect(s.page).toBe("ecosystem");
    expect(s.toasts[0]?.text).toContain("removed this computer");
  });

  it("discovery ignores paired devices and forgets lost ones", () => {
    const st = useStore.getState();
    const d = {
      id: "N",
      name: "N",
      kind: "laptop" as const,
      platform: "linux" as const,
      addrs: [],
      fingerprint: null,
      pairing_open: true,
    };
    st.apply({ type: "device_discovered", device: d });
    expect(useStore.getState().nearby.N).toBeDefined();
    st.apply({ type: "device_lost", id: "N" });
    expect(useStore.getState().nearby.N).toBeUndefined();
    st.apply({ type: "peer_changed", peer: peer("N", true) });
    st.apply({ type: "device_discovered", device: d });
    expect(useStore.getState().nearby.N).toBeUndefined();
  });

  it("pause and local updates", () => {
    const st = useStore.getState();
    st.apply({ type: "paused", paused: true });
    expect(useStore.getState().local?.paused).toBe(true);
    st.apply({ type: "local_changed", local: { ...local, info: { ...local.info, name: "New" } } });
    expect(useStore.getState().local?.info.name).toBe("New");
  });

  it("orders online devices first, then by pairing time", () => {
    const list = orderedPeers({ a: peer("a", false, 1), b: peer("b", true, 3), c: peer("c", true, 2) });
    expect(list.map((p) => p.info.id)).toEqual(["c", "b", "a"]);
  });
});
