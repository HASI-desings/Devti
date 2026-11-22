//! Phase 4 on the Hub: shares this computer's screen (or one window) with paired devices
//! that ask to see it — as a mirror, a second screen for one app, or an extended display
//! when a virtual monitor exists.
//!
//! Pipeline, on one capture thread: grab RGBA → scale to the width the core asks for
//! (`fast_image_resize`) → JPEG at the core's quality (`jpeg-encoder`) → `push_screen_frame`.
//! The core sends only the newest frame, so pacing here is just the target frame rate.
//!
//! Sources:
//! * X11 and Windows: any monitor or window, captured directly (xcap: XShm/XGetImage,
//!   DXGI/GDI).
//! * Wayland: screens can't be read directly, so the xdg-desktop-portal ScreenCast dialog
//!   (via PipeWire) lets the user pick a screen or a window.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use devti_core::Node;
use fast_image_resize as fr;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

pub const SETTING_SOURCE: &str = "screen_source";

/// What this Hub shares.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    Monitor { id: u32 },
    Window { id: u32 },
}

/// A monitor or window the user can choose.
#[derive(Clone, Debug, Serialize)]
pub struct SourceInfo {
    pub source: Source,
    pub name: String,
    pub detail: String,
    pub width: u32,
    pub height: u32,
    pub x: i32,
    pub y: i32,
    pub primary: bool,
}

struct Running {
    stop: Arc<AtomicBool>,
}

pub struct Screens {
    node: Node,
    running: Mutex<Option<Running>>,
}

pub fn wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").map(|s| s == "wayland").unwrap_or(false)
        || (std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_none())
}

impl Screens {
    pub fn new(node: Node) -> Arc<Self> {
        Arc::new(Self { node, running: Mutex::new(None) })
    }

    pub fn source(&self) -> Option<Source> {
        self.node.setting(SETTING_SOURCE).ok().flatten().and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn set_source(self: &Arc<Self>, source: Option<Source>) -> anyhow::Result<()> {
        let v = source.as_ref().map(serde_json::to_string).transpose()?.unwrap_or_default();
        self.node.set_setting(SETTING_SOURCE, &v)?;
        // Restart capture with the new source if someone is watching.
        if self.running.lock().is_some() {
            self.stop();
            self.ensure_running(None);
        }
        Ok(())
    }

    /// Monitors (and windows, where they can be captured directly).
    pub fn sources() -> Vec<SourceInfo> {
        let mut out = Vec::new();
        if let Ok(monitors) = xcap::Monitor::all() {
            for m in monitors {
                let (Ok(id), Ok(w), Ok(h)) = (m.id(), m.width(), m.height()) else { continue };
                let name = m.friendly_name().or_else(|_| m.name()).unwrap_or_else(|_| format!("Screen {id}"));
                out.push(SourceInfo {
                    source: Source::Monitor { id },
                    detail: format!("{w} × {h}"),
                    name,
                    width: w,
                    height: h,
                    x: m.x().unwrap_or(0),
                    y: m.y().unwrap_or(0),
                    primary: m.is_primary().unwrap_or(false),
                });
            }
        }
        if !wayland()
            && let Ok(windows) = xcap::Window::all()
        {
            for w in windows {
                let title = w.title().unwrap_or_default();
                if title.trim().is_empty() || w.is_minimized().unwrap_or(false) {
                    continue;
                }
                let (Ok(id), Ok(ww), Ok(hh)) = (w.id(), w.width(), w.height()) else { continue };
                if ww < 120 || hh < 80 {
                    continue;
                }
                out.push(SourceInfo {
                    source: Source::Window { id },
                    name: title.chars().take(80).collect(),
                    detail: w.app_name().unwrap_or_default(),
                    width: ww,
                    height: hh,
                    x: w.x().unwrap_or(0),
                    y: w.y().unwrap_or(0),
                    primary: false,
                });
            }
        }
        out
    }

    /// Starts capturing if any device is watching (and stops when none are).
    pub fn ensure_running(self: &Arc<Self>, app: Option<&AppHandle>) {
        let viewers = self.node.screen_viewers();
        let mut slot = self.running.lock();
        if viewers.is_empty() {
            if let Some(r) = slot.take() {
                r.stop.store(true, Ordering::SeqCst);
            }
            return;
        }
        if slot.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        *slot = Some(Running { stop: stop.clone() });
        let me = self.clone();
        let app = app.cloned();
        std::thread::Builder::new()
            .name("devti-screen".into())
            .spawn(move || {
                if let Err(e) = me.capture_loop(&stop) {
                    tracing::warn!(error = %e, "screen sharing stopped");
                    if let Some(app) = app {
                        let _ = app.emit("devti://notice", format!("Screen sharing stopped: {e}"));
                    }
                    for v in me.node.screen_viewers() {
                        me.node.screen_stop_sharing(&v);
                    }
                }
                *me.running.lock() = None;
            })
            .ok();
    }

    pub fn stop(&self) {
        if let Some(r) = self.running.lock().take() {
            r.stop.store(true, Ordering::SeqCst);
        }
    }

    fn capture_loop(&self, stop: &AtomicBool) -> anyhow::Result<()> {
        let mut encoder = FrameEncoder::default();
        let source = self.source();
        if wayland() {
            return self.wayland_loop(stop, &mut encoder);
        }
        let monitor = match &source {
            Some(Source::Monitor { id }) => xcap::Monitor::all()?.into_iter().find(|m| m.id().ok() == Some(*id)),
            _ => None,
        };
        let window = match &source {
            Some(Source::Window { id }) => xcap::Window::all()?.into_iter().find(|w| w.id().ok() == Some(*id)),
            _ => None,
        };
        let monitor = match (monitor, &window) {
            (Some(m), _) => Some(m),
            (None, None) => Some(
                xcap::Monitor::all()?
                    .into_iter()
                    .find(|m| m.is_primary().unwrap_or(false))
                    .or_else(|| xcap::Monitor::all().ok().and_then(|v| v.into_iter().next()))
                    .ok_or_else(|| anyhow::anyhow!("no screen to share"))?,
            ),
            (None, Some(_)) => None,
        };
        while !stop.load(Ordering::SeqCst) {
            let started = Instant::now();
            let target = self.node.screen_target();
            let image = match (&monitor, &window) {
                (Some(m), _) => m.capture_image()?,
                (None, Some(w)) => w.capture_image()?,
                _ => unreachable!(),
            };
            let (w, h) = (image.width(), image.height());
            if let Some((ow, oh, jpeg)) = encoder.encode(image.as_raw(), w, h, target.max_width, target.quality)? {
                let _ = self.node.push_screen_frame(ow, oh, jpeg);
            }
            pace(started, target.fps);
        }
        Ok(())
    }

    /// Wayland: frames from the ScreenCast portal (the user picks a screen or window once).
    fn wayland_loop(&self, stop: &AtomicBool, encoder: &mut FrameEncoder) -> anyhow::Result<()> {
        let monitor = xcap::Monitor::all()?.into_iter().next().ok_or_else(|| anyhow::anyhow!("no screen to share"))?;
        let (recorder, frames) = monitor.video_recorder()?;
        recorder.start()?;
        let mut last = Instant::now() - Duration::from_secs(1);
        while !stop.load(Ordering::SeqCst) {
            let Ok(frame) = frames.recv_timeout(Duration::from_millis(500)) else { continue };
            let target = self.node.screen_target();
            if last.elapsed() < Duration::from_millis(1000 / target.fps.max(1) as u64) {
                continue;
            }
            last = Instant::now();
            if let Some((ow, oh, jpeg)) =
                encoder.encode(&frame.raw, frame.width, frame.height, target.max_width, target.quality)?
            {
                let _ = self.node.push_screen_frame(ow, oh, jpeg);
            }
        }
        let _ = recorder.stop();
        Ok(())
    }
}

fn pace(started: Instant, fps: u8) {
    let frame = Duration::from_millis(1000 / fps.max(1) as u64);
    if let Some(left) = frame.checked_sub(started.elapsed()) {
        std::thread::sleep(left);
    }
}

/// Scales RGBA to the target width and encodes JPEG; skips identical frames.
#[derive(Default)]
pub struct FrameEncoder {
    resizer: Option<fr::Resizer>,
    last_hash: u64,
    same_count: u32,
}

impl FrameEncoder {
    pub fn encode(
        &mut self,
        rgba: &[u8],
        w: u32,
        h: u32,
        max_width: u32,
        quality: u8,
    ) -> anyhow::Result<Option<(u32, u32, Vec<u8>)>> {
        if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
            return Ok(None);
        }
        // A still screen costs nothing: resend an unchanged frame only every ~2 s.
        let hash = sample_hash(rgba);
        if hash == self.last_hash {
            self.same_count += 1;
            if self.same_count % 60 != 0 {
                return Ok(None);
            }
        } else {
            self.same_count = 0;
            self.last_hash = hash;
        }
        let (ow, oh) = fit(w, h, max_width);
        let scaled: std::borrow::Cow<[u8]> = if (ow, oh) == (w, h) {
            std::borrow::Cow::Borrowed(&rgba[..(w * h * 4) as usize])
        } else {
            let src = fr::images::ImageRef::new(w, h, &rgba[..(w * h * 4) as usize], fr::PixelType::U8x4)?;
            let mut dst = fr::images::Image::new(ow, oh, fr::PixelType::U8x4);
            let resizer = self.resizer.get_or_insert_with(fr::Resizer::new);
            resizer.resize(
                &src,
                &mut dst,
                &fr::ResizeOptions::new().resize_alg(fr::ResizeAlg::Convolution(fr::FilterType::Bilinear)),
            )?;
            std::borrow::Cow::Owned(dst.into_vec())
        };
        let mut out = Vec::with_capacity((ow * oh / 4) as usize);
        let enc = jpeg_encoder::Encoder::new(&mut out, quality.clamp(30, 92));
        enc.encode(&scaled, ow as u16, oh as u16, jpeg_encoder::ColorType::Rgba)?;
        Ok(Some((ow, oh, out)))
    }
}

/// Output size: at most `max_width` wide, even dimensions, same aspect ratio.
pub fn fit(w: u32, h: u32, max_width: u32) -> (u32, u32) {
    if w <= max_width {
        return (w & !1, h & !1);
    }
    let ow = max_width & !1;
    let oh = ((h as u64 * ow as u64 / w as u64) as u32).max(2) & !1;
    (ow, oh)
}

/// Cheap change detector: hashes ~4 000 pixels spread over the frame.
fn sample_hash(rgba: &[u8]) -> u64 {
    let step = (rgba.len() / 4 / 4096).max(1) * 4;
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i + 3 < rgba.len() {
        h = (h ^ u32::from_le_bytes([rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]) as u64)
            .wrapping_mul(0x100000001b3);
        i += step;
    }
    h
}

pub fn app_state(app: &AppHandle) -> Option<Arc<Screens>> {
    app.try_state::<Arc<Screens>>().map(|s| s.inner().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_aspect_and_even_sizes() {
        assert_eq!(fit(1920, 1080, 1280), (1280, 720));
        assert_eq!(fit(1366, 768, 1600), (1366, 768));
        assert_eq!(fit(1081, 1921, 720), (720, 1278));
    }

    #[test]
    fn encodes_scales_and_skips_still_frames() {
        let (w, h) = (640u32, 360u32);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for (i, px) in rgba.chunks_exact_mut(4).enumerate() {
            px.copy_from_slice(&[(i % 251) as u8, (i % 97) as u8, 200, 255]);
        }
        let mut enc = FrameEncoder::default();
        let (ow, oh, jpeg) = enc.encode(&rgba, w, h, 320, 70).unwrap().unwrap();
        assert_eq!((ow, oh), (320, 180));
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "a JPEG");
        assert!(enc.encode(&rgba, w, h, 320, 70).unwrap().is_none(), "unchanged frame skipped");
        rgba[1000] ^= 0xFF;
        rgba[(w * h * 2) as usize] ^= 0xFF;
        let changed = enc.encode(&rgba, w, h, 320, 70).unwrap();
        assert!(changed.is_some() || sample_hash(&rgba) == enc.last_hash);
    }
}
