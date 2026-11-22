//! Phase 4 "one keyboard and mouse": push the pointer past an edge of this computer's
//! screens and it continues on the paired computer placed there (Deskflow-style). The
//! keyboard follows. Come back by moving past the same edge the other way, or press
//! Scroll Lock.
//!
//! * The geometry and the return condition are platform-free and unit-tested.
//! * X11: polls the pointer; on an edge it grabs pointer and keyboard (XGrabPointer /
//!   XGrabKeyboard), keeps the real pointer parked in the middle, and forwards relative
//!   motion, buttons, wheel and keys.
//! * Windows: low-level mouse and keyboard hooks swallow input while the other computer
//!   has it.
//! * Wayland: compositors don't let apps capture input yet (the InputCapture portal is
//!   too new to rely on), so the Screens page says it's unavailable there.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use devti_core::Node;
use devti_core::model::DeviceId;
use devti_core::protocol::{InputEvent, MouseButton};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

pub const SETTING_LAYOUT: &str = "screen_layout";
/// Pixels to move back past the edge before control returns.
const EXIT_PX: f64 = 3.0;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// A paired computer placed next to one edge of this computer's screens.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Link {
    pub device: String,
    pub edge: Edge,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Layout {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub links: Vec<Link>,
}

/// The bounding box of all this computer's screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Layout {
    /// Which linked edge the pointer is touching, if any.
    pub fn hit(&self, b: Bounds, x: i32, y: i32) -> Option<(Edge, &Link)> {
        let edge = if x <= b.x {
            Edge::Left
        } else if x >= b.x + b.w - 1 {
            Edge::Right
        } else if y <= b.y {
            Edge::Top
        } else if y >= b.y + b.h - 1 {
            Edge::Bottom
        } else {
            return None;
        };
        self.links.iter().find(|l| l.edge == edge).map(|l| (edge, l))
    }
}

/// Where the pointer appears on the other computer (fractions), mirroring where it left.
pub fn entry_point(edge: Edge, b: Bounds, x: i32, y: i32) -> (f32, f32) {
    let fx = ((x - b.x) as f32 / b.w.max(1) as f32).clamp(0.0, 1.0);
    let fy = ((y - b.y) as f32 / b.h.max(1) as f32).clamp(0.0, 1.0);
    match edge {
        Edge::Right => (0.0, fy),
        Edge::Left => (1.0, fy),
        Edge::Bottom => (fx, 0.0),
        Edge::Top => (fx, 1.0),
    }
}

/// Where to put the local pointer when control comes back.
pub fn return_point(edge: Edge, b: Bounds, x: i32, y: i32) -> (i32, i32) {
    const IN: i32 = 24;
    match edge {
        Edge::Right => (b.x + b.w - IN, y),
        Edge::Left => (b.x + IN, y),
        Edge::Bottom => (x, b.y + b.h - IN),
        Edge::Top => (x, b.y + IN),
    }
}

/// Tracks how far into the other computer the pointer has gone along the crossing axis.
#[derive(Debug)]
pub struct Excursion {
    edge: Edge,
    depth: f64,
}

impl Excursion {
    pub fn new(edge: Edge) -> Self {
        Self { edge, depth: 0.0 }
    }

    /// Adds a relative move; returns true when the pointer came back across the edge.
    pub fn moved(&mut self, dx: f64, dy: f64) -> bool {
        self.depth += match self.edge {
            Edge::Right => dx,
            Edge::Left => -dx,
            Edge::Bottom => dy,
            Edge::Top => -dy,
        };
        // The other computer clamps its pointer at its edge; we don't need its size.
        self.depth = self.depth.max(-EXIT_PX - 1.0);
        self.depth < -EXIT_PX
    }
}

/// Shared state between the UI commands and the platform loop.
pub struct Edges {
    pub node: Node,
    layout: RwLock<Layout>,
    pub active: AtomicBool,
}

impl Edges {
    pub fn new(node: Node) -> Arc<Self> {
        let layout =
            node.setting(SETTING_LAYOUT).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Arc::new(Self { node, layout: RwLock::new(layout), active: AtomicBool::new(false) })
    }

    pub fn layout(&self) -> Layout {
        self.layout.read().clone()
    }

    pub fn set_layout(&self, layout: Layout) -> anyhow::Result<()> {
        self.node.set_setting(SETTING_LAYOUT, &serde_json::to_string(&layout)?)?;
        *self.layout.write() = layout;
        Ok(())
    }

    /// Only watch the pointer when it's on and a linked device is online.
    pub fn should_watch(&self) -> bool {
        let l = self.layout.read();
        l.enabled && l.links.iter().any(|link| self.online(&link.device).is_some())
    }

    pub fn online(&self, device: &str) -> Option<DeviceId> {
        let id: DeviceId = device.parse().ok()?;
        self.node.peer(&id).filter(|p| p.online).map(|p| p.info.id)
    }

    pub fn send(&self, to: &DeviceId, events: Vec<InputEvent>) {
        let _ = self.node.send_input(to, events);
    }

    pub fn supported() -> bool {
        cfg!(windows)
            || (cfg!(target_os = "linux") && !crate::screens::wayland() && std::env::var_os("DISPLAY").is_some())
    }

    /// Starts the platform loop (once).
    pub fn start(self: &Arc<Self>) {
        if !Self::supported() {
            return;
        }
        let me = self.clone();
        std::thread::Builder::new()
            .name("devti-edges".into())
            .spawn(move || {
                if let Err(e) = platform::run(me) {
                    tracing::warn!(error = %e, "one keyboard and mouse stopped");
                }
            })
            .ok();
    }
}

pub fn button_of(n: u8) -> Option<MouseButton> {
    match n {
        1 => Some(MouseButton::Left),
        2 => Some(MouseButton::Middle),
        3 => Some(MouseButton::Right),
        _ => None,
    }
}

// -------------------------------------------------------------------- X11

#[cfg(target_os = "linux")]
mod platform {
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use devti_core::protocol::InputEvent;
    use x11rb::connection::Connection;
    use x11rb::protocol::Event;
    use x11rb::protocol::xproto::{ConnectionExt, EventMask, GrabMode, GrabStatus, KeyButMask};
    use x11rb::rust_connection::RustConnection;

    use super::{Bounds, Edges, Excursion, button_of, entry_point, return_point};

    pub fn run(edges: Arc<Edges>) -> anyhow::Result<()> {
        let (conn, screen_num) = RustConnection::connect(None)?;
        let screen = conn.setup().roots[screen_num].clone();
        let root = screen.root;
        let b = Bounds { x: 0, y: 0, w: screen.width_in_pixels as i32, h: screen.height_in_pixels as i32 };
        let (min_kc, max_kc) = (conn.setup().min_keycode, conn.setup().max_keycode);
        let mapping = conn.get_keyboard_mapping(min_kc, max_kc - min_kc + 1)?.reply()?;
        let per = mapping.keysyms_per_keycode as usize;
        let keysym = |code: u8, shift: bool| -> u32 {
            let base = (code.saturating_sub(min_kc)) as usize * per;
            let s = mapping.keysyms.get(base + usize::from(shift && per > 1)).copied().unwrap_or(0);
            if s == 0 { mapping.keysyms.get(base).copied().unwrap_or(0) } else { s }
        };
        loop {
            if !edges.should_watch() {
                std::thread::sleep(Duration::from_millis(400));
                continue;
            }
            std::thread::sleep(Duration::from_millis(8));
            let p = conn.query_pointer(root)?.reply()?;
            let (x, y) = (p.root_x as i32, p.root_y as i32);
            let layout = edges.layout();
            let Some((edge, link)) = layout.hit(b, x, y) else { continue };
            let Some(target) = edges.online(&link.device) else { continue };

            let grab = conn
                .grab_pointer(
                    false,
                    root,
                    EventMask::POINTER_MOTION | EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE,
                    GrabMode::ASYNC,
                    GrabMode::ASYNC,
                    root,
                    x11rb::NONE,
                    x11rb::CURRENT_TIME,
                )?
                .reply()?;
            if grab.status != GrabStatus::SUCCESS {
                continue;
            }
            let _ = conn.grab_keyboard(false, root, x11rb::CURRENT_TIME, GrabMode::ASYNC, GrabMode::ASYNC)?.reply();
            let (cx, cy) = (b.w / 2, b.h / 2);
            conn.warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, cx as i16, cy as i16)?;
            conn.flush()?;
            edges.active.store(true, Ordering::SeqCst);
            let (fx, fy) = entry_point(edge, b, x, y);
            edges.send(&target, vec![InputEvent::MoveTo { x: fx, y: fy }]);
            let mut trip = Excursion::new(edge);

            'remote: loop {
                if edges.online(&link.device).is_none() || !edges.layout().enabled {
                    break 'remote;
                }
                let Some(ev) = conn.poll_for_event()? else {
                    std::thread::sleep(Duration::from_millis(3));
                    continue;
                };
                match ev {
                    Event::MotionNotify(m) => {
                        let (dx, dy) = (m.root_x as i32 - cx, m.root_y as i32 - cy);
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        conn.warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, cx as i16, cy as i16)?;
                        conn.flush()?;
                        if trip.moved(dx as f64, dy as f64) {
                            break 'remote;
                        }
                        edges.send(&target, vec![InputEvent::Move { dx: dx as f32, dy: dy as f32 }]);
                    }
                    Event::ButtonPress(e) => match e.detail {
                        4 => edges.send(&target, vec![InputEvent::Scroll { dx: 0.0, dy: -1.0 }]),
                        5 => edges.send(&target, vec![InputEvent::Scroll { dx: 0.0, dy: 1.0 }]),
                        6 => edges.send(&target, vec![InputEvent::Scroll { dx: -1.0, dy: 0.0 }]),
                        7 => edges.send(&target, vec![InputEvent::Scroll { dx: 1.0, dy: 0.0 }]),
                        n => {
                            if let Some(button) = button_of(n) {
                                edges.send(&target, vec![InputEvent::Button { button, down: true }]);
                            }
                        }
                    },
                    Event::ButtonRelease(e) => {
                        if let Some(button) = button_of(e.detail) {
                            edges.send(&target, vec![InputEvent::Button { button, down: false }]);
                        }
                    }
                    Event::KeyPress(k) => {
                        let state = u16::from(k.state);
                        let shift = state & u16::from(KeyButMask::SHIFT) != 0;
                        let sym = keysym(k.detail, shift);
                        if sym == 0xff14 {
                            // Scroll Lock: take the pointer back.
                            break 'remote;
                        }
                        let mods = super::keys::mods(
                            state & u16::from(KeyButMask::CONTROL) != 0,
                            state & u16::from(KeyButMask::MOD1) != 0,
                            state & u16::from(KeyButMask::MOD4) != 0,
                            shift,
                        );
                        if let Some(ev) = super::keys::from_keysym(sym, &mods) {
                            edges.send(&target, vec![ev]);
                        }
                    }
                    _ => {}
                }
            }
            conn.ungrab_pointer(x11rb::CURRENT_TIME)?;
            conn.ungrab_keyboard(x11rb::CURRENT_TIME)?;
            let (rx, ry) = return_point(edge, b, x, y);
            conn.warp_pointer(x11rb::NONE, root, 0, 0, 0, 0, rx as i16, ry as i16)?;
            conn.flush()?;
            edges.active.store(false, Ordering::SeqCst);
        }
    }
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
mod platform {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
    use std::time::Duration;

    use devti_core::protocol::{InputEvent, MouseButton};
    use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::ToUnicode;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, KBDLLHOOKSTRUCT, MSG,
        MSLLHOOKSTRUCT, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SetCursorPos,
        SetWindowsHookExW, TranslateMessage, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
        WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN,
        WM_RBUTTONUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    use super::{Bounds, Edges, Excursion, entry_point, return_point};

    enum Hooked {
        Move(i32, i32),
        Button(MouseButton, bool),
        Wheel(f32, f32),
        Key { vk: u32, scan: u32 },
        Release,
    }

    static REMOTE: AtomicBool = AtomicBool::new(false);
    static CX: AtomicI32 = AtomicI32::new(0);
    static CY: AtomicI32 = AtomicI32::new(0);
    static TX: OnceLock<Mutex<std::sync::mpsc::Sender<Hooked>>> = OnceLock::new();
    // Modifier state seen by the keyboard hook (the OS state is frozen while we swallow keys).
    static CTRL: AtomicBool = AtomicBool::new(false);
    static ALT: AtomicBool = AtomicBool::new(false);
    static WIN: AtomicBool = AtomicBool::new(false);
    static SHIFT: AtomicBool = AtomicBool::new(false);
    const LLMHF_INJECTED: u32 = 0x1;

    fn push(h: Hooked) {
        if let Some(tx) = TX.get()
            && let Ok(tx) = tx.lock()
        {
            let _ = tx.send(h);
        }
    }

    unsafe extern "system" fn mouse_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code >= 0 && REMOTE.load(Ordering::SeqCst) {
            // SAFETY: for WH_MOUSE_LL, lParam points to an MSLLHOOKSTRUCT owned by the system.
            let info = unsafe { &*(l.0 as *const MSLLHOOKSTRUCT) };
            if info.flags & LLMHF_INJECTED == 0 {
                let wheel = ((info.mouseData >> 16) as i16) as f32 / 120.0;
                match w.0 as u32 {
                    WM_MOUSEMOVE => {
                        let (cx, cy) = (CX.load(Ordering::SeqCst), CY.load(Ordering::SeqCst));
                        push(Hooked::Move(info.pt.x - cx, info.pt.y - cy));
                    }
                    WM_LBUTTONDOWN => push(Hooked::Button(MouseButton::Left, true)),
                    WM_LBUTTONUP => push(Hooked::Button(MouseButton::Left, false)),
                    WM_RBUTTONDOWN => push(Hooked::Button(MouseButton::Right, true)),
                    WM_RBUTTONUP => push(Hooked::Button(MouseButton::Right, false)),
                    WM_MBUTTONDOWN => push(Hooked::Button(MouseButton::Middle, true)),
                    WM_MBUTTONUP => push(Hooked::Button(MouseButton::Middle, false)),
                    WM_MOUSEWHEEL => push(Hooked::Wheel(0.0, -wheel)),
                    WM_MOUSEHWHEEL => push(Hooked::Wheel(wheel, 0.0)),
                    _ => {}
                }
                return LRESULT(1);
            }
        }
        unsafe { CallNextHookEx(None, code, w, l) }
    }

    unsafe extern "system" fn keyboard_proc(code: i32, w: WPARAM, l: LPARAM) -> LRESULT {
        if code >= 0 && REMOTE.load(Ordering::SeqCst) {
            // SAFETY: for WH_KEYBOARD_LL, lParam points to a KBDLLHOOKSTRUCT owned by the system.
            let info = unsafe { &*(l.0 as *const KBDLLHOOKSTRUCT) };
            let down = matches!(w.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            let up = matches!(w.0 as u32, WM_KEYUP | WM_SYSKEYUP);
            match info.vkCode {
                0x10 | 0xA0 | 0xA1 => SHIFT.store(down || !up && SHIFT.load(Ordering::SeqCst), Ordering::SeqCst),
                0x11 | 0xA2 | 0xA3 => CTRL.store(down || !up && CTRL.load(Ordering::SeqCst), Ordering::SeqCst),
                0x12 | 0xA4 | 0xA5 => ALT.store(down || !up && ALT.load(Ordering::SeqCst), Ordering::SeqCst),
                0x5B | 0x5C => WIN.store(down || !up && WIN.load(Ordering::SeqCst), Ordering::SeqCst),
                0x91 if down => push(Hooked::Release), // Scroll Lock
                vk if down => push(Hooked::Key { vk, scan: info.scanCode }),
                _ => {}
            }
            return LRESULT(1);
        }
        unsafe { CallNextHookEx(None, code, w, l) }
    }

    fn bounds() -> Bounds {
        // SAFETY: plain metric queries.
        unsafe {
            Bounds {
                x: GetSystemMetrics(SM_XVIRTUALSCREEN),
                y: GetSystemMetrics(SM_YVIRTUALSCREEN),
                w: GetSystemMetrics(SM_CXVIRTUALSCREEN),
                h: GetSystemMetrics(SM_CYVIRTUALSCREEN),
            }
        }
    }

    fn key_event(vk: u32, scan: u32) -> Option<InputEvent> {
        let mods = super::keys::mods(
            CTRL.load(Ordering::SeqCst),
            ALT.load(Ordering::SeqCst),
            WIN.load(Ordering::SeqCst),
            SHIFT.load(Ordering::SeqCst),
        );
        if let Some(name) = super::keys::from_vk(vk) {
            return Some(InputEvent::Key { key: name.to_string(), mods });
        }
        let mut state = [0u8; 256];
        if SHIFT.load(Ordering::SeqCst) {
            state[0x10] = 0x80;
        }
        let mut buf = [0u16; 4];
        // SAFETY: valid buffers; translation only, no side effects on the system state.
        let n = unsafe { ToUnicode(vk, scan, Some(&state), &mut buf, 0) };
        if n <= 0 {
            return None;
        }
        let text = String::from_utf16_lossy(&buf[..n as usize]);
        if mods.iter().any(|m| m != "shift") {
            Some(InputEvent::Key { key: text.to_lowercase(), mods })
        } else {
            Some(InputEvent::Text { text })
        }
    }

    pub fn run(edges: Arc<Edges>) -> anyhow::Result<()> {
        let (tx, rx) = std::sync::mpsc::channel();
        let _ = TX.set(Mutex::new(tx));
        // Hooks live on their own thread with a message loop.
        std::thread::Builder::new().name("devti-hooks".into()).spawn(|| unsafe {
            let module = GetModuleHandleW(None).ok();
            let _m = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), module.map(Into::into), 0);
            let _k = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), module.map(Into::into), 0);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })?;
        loop {
            if !edges.should_watch() {
                std::thread::sleep(Duration::from_millis(400));
                continue;
            }
            std::thread::sleep(Duration::from_millis(8));
            let b = bounds();
            let mut p = POINT::default();
            // SAFETY: writes the cursor position into `p`.
            if unsafe { GetCursorPos(&mut p) }.is_err() {
                continue;
            }
            let layout = edges.layout();
            let Some((edge, link)) = layout.hit(b, p.x, p.y) else { continue };
            let Some(target) = edges.online(&link.device) else { continue };
            let (cx, cy) = (b.x + b.w / 2, b.y + b.h / 2);
            CX.store(cx, Ordering::SeqCst);
            CY.store(cy, Ordering::SeqCst);
            while rx.try_recv().is_ok() {}
            REMOTE.store(true, Ordering::SeqCst);
            edges.active.store(true, Ordering::SeqCst);
            // SAFETY: moves the cursor; no memory involved.
            let _ = unsafe { SetCursorPos(cx, cy) };
            let (fx, fy) = entry_point(edge, b, p.x, p.y);
            edges.send(&target, vec![InputEvent::MoveTo { x: fx, y: fy }]);
            let mut trip = Excursion::new(edge);
            loop {
                if edges.online(&link.device).is_none() || !edges.layout().enabled {
                    break;
                }
                let Ok(h) = rx.recv_timeout(Duration::from_millis(200)) else { continue };
                match h {
                    Hooked::Move(dx, dy) => {
                        if dx == 0 && dy == 0 {
                            continue;
                        }
                        let _ = unsafe { SetCursorPos(cx, cy) };
                        if trip.moved(dx as f64, dy as f64) {
                            break;
                        }
                        edges.send(&target, vec![InputEvent::Move { dx: dx as f32, dy: dy as f32 }]);
                    }
                    Hooked::Button(button, down) => edges.send(&target, vec![InputEvent::Button { button, down }]),
                    Hooked::Wheel(dx, dy) => edges.send(&target, vec![InputEvent::Scroll { dx, dy }]),
                    Hooked::Key { vk, scan } => {
                        if let Some(ev) = key_event(vk, scan) {
                            edges.send(&target, vec![ev]);
                        }
                    }
                    Hooked::Release => break,
                }
            }
            REMOTE.store(false, Ordering::SeqCst);
            edges.active.store(false, Ordering::SeqCst);
            let (rx_, ry_) = return_point(edge, b, p.x, p.y);
            let _ = unsafe { SetCursorPos(rx_, ry_) };
        }
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    pub fn run(_: std::sync::Arc<super::Edges>) -> anyhow::Result<()> {
        Ok(())
    }
}

/// Key translation shared by both platforms.
pub mod keys {
    use devti_core::protocol::InputEvent;

    pub fn mods(ctrl: bool, alt: bool, meta: bool, shift: bool) -> Vec<String> {
        let mut m = Vec::new();
        if ctrl {
            m.push("ctrl".to_string());
        }
        if alt {
            m.push("alt".to_string());
        }
        if meta {
            m.push("meta".to_string());
        }
        if shift {
            m.push("shift".to_string());
        }
        m
    }

    /// X11 keysym → input event (named key, shortcut or text).
    pub fn from_keysym(sym: u32, mods: &[String]) -> Option<InputEvent> {
        let named = match sym {
            0xff0d | 0xff8d => "enter",
            0xff08 => "backspace",
            0xff09 | 0xfe20 => "tab",
            0xff1b => "escape",
            0xffff => "delete",
            0xff63 => "insert",
            0xff50 => "home",
            0xff57 => "end",
            0xff55 => "page_up",
            0xff56 => "page_down",
            0xff51 => "left",
            0xff52 => "up",
            0xff53 => "right",
            0xff54 => "down",
            0xff61 => "print_screen",
            s @ 0xffbe..=0xffc9 => {
                return Some(InputEvent::Key { key: format!("f{}", s - 0xffbe + 1), mods: mods.to_vec() });
            }
            // Modifier keys alone travel with the next key.
            0xffe1..=0xffee => return None,
            _ => "",
        };
        if !named.is_empty() {
            return Some(InputEvent::Key { key: named.into(), mods: mods.to_vec() });
        }
        let ch = xkeysym::Keysym::from(sym).key_char()?;
        if ch.is_control() {
            return None;
        }
        if mods.iter().any(|m| m != "shift") {
            Some(InputEvent::Key { key: ch.to_lowercase().to_string(), mods: mods.to_vec() })
        } else {
            Some(InputEvent::Text { text: ch.to_string() })
        }
    }

    /// Windows virtual-key → named key (printable keys go through ToUnicode).
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn from_vk(vk: u32) -> Option<&'static str> {
        Some(match vk {
            0x0D => "enter",
            0x08 => "backspace",
            0x09 => "tab",
            0x1B => "escape",
            0x2E => "delete",
            0x2D => "insert",
            0x24 => "home",
            0x23 => "end",
            0x21 => "page_up",
            0x22 => "page_down",
            0x25 => "left",
            0x26 => "up",
            0x27 => "right",
            0x28 => "down",
            0x2C => "print_screen",
            0x70 => "f1",
            0x71 => "f2",
            0x72 => "f3",
            0x73 => "f4",
            0x74 => "f5",
            0x75 => "f6",
            0x76 => "f7",
            0x77 => "f8",
            0x78 => "f9",
            0x79 => "f10",
            0x7A => "f11",
            0x7B => "f12",
            _ => return None,
        })
    }
}

/// Whether the other computer currently has the pointer (for the UI).
pub fn is_active(e: &Edges) -> bool {
    e.active.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    const B: Bounds = Bounds { x: 0, y: 0, w: 1920, h: 1080 };

    #[test]
    fn edges_hit_only_where_linked() {
        let l = Layout { enabled: true, links: vec![Link { device: "A".into(), edge: Edge::Right }] };
        assert_eq!(l.hit(B, 1919, 500).map(|(e, _)| e), Some(Edge::Right));
        assert!(l.hit(B, 0, 500).is_none(), "left edge isn't linked");
        assert!(l.hit(B, 900, 500).is_none());
    }

    #[test]
    fn entry_mirrors_the_exit_point() {
        assert_eq!(entry_point(Edge::Right, B, 1919, 540), (0.0, 0.5));
        assert_eq!(entry_point(Edge::Top, B, 960, 0), (0.5, 1.0));
        assert_eq!(return_point(Edge::Right, B, 1919, 300), (1896, 300));
    }

    #[test]
    fn coming_back_needs_moving_past_the_edge() {
        let mut t = Excursion::new(Edge::Right);
        assert!(!t.moved(400.0, 0.0));
        assert!(!t.moved(-399.0, 0.0), "still 1 px inside the other screen");
        assert!(!t.moved(-3.0, 30.0));
        assert!(t.moved(-2.0, 0.0), "now past the edge");
        let mut up = Excursion::new(Edge::Top);
        assert!(!up.moved(0.0, -50.0));
        assert!(up.moved(0.0, 54.0));
    }

    #[test]
    fn keys_translate() {
        let none: Vec<String> = vec![];
        assert_eq!(keys::from_keysym(0x61, &none), Some(InputEvent::Text { text: "a".into() }));
        assert_eq!(
            keys::from_keysym(0x63, &keys::mods(true, false, false, false)),
            Some(InputEvent::Key { key: "c".into(), mods: vec!["ctrl".into()] })
        );
        assert_eq!(keys::from_keysym(0xff0d, &none), Some(InputEvent::Key { key: "enter".into(), mods: vec![] }));
        assert_eq!(keys::from_keysym(0xffc2, &none), Some(InputEvent::Key { key: "f5".into(), mods: vec![] }));
        assert_eq!(keys::from_keysym(0xffe1, &none), None, "Shift alone");
        assert_eq!(keys::from_vk(0x74), Some("f5"));
        assert_eq!(keys::from_vk(0x41), None, "letters go through ToUnicode");
    }
}
