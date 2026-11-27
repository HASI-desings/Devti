//! Recording a phone's microphone or sound on this computer (1.1).
//!
//! The Hub asks the phone for its audio (`AudioRequest`); the phone asks its user, as
//! Android requires, then streams PCM. Every chunk goes into a WAV file in
//! `Music/Devti`, named after the device and the time, until the user stops.

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;
use std::time::Instant;

use serde::Serialize;

pub struct Recording {
    path: PathBuf,
    writer: Option<hound::WavWriter<BufWriter<File>>>,
    asked: Instant,
    started: Option<Instant>,
    samples: u64,
    channels: u16,
    rate: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecordingState {
    pub device: String,
    /// "mic" or "relay".
    pub kind: String,
    /// The phone allowed it and audio is arriving.
    pub live: bool,
    /// Seconds recorded so far.
    pub seconds: f64,
    pub path: String,
}

/// `Music/Devti`, created on demand.
pub fn folder() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    home.join("Music").join("Devti")
}

fn safe(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' })
        .collect::<String>()
        .trim()
        .to_string()
}

impl Recording {
    /// A recording waiting for the phone's audio to start.
    pub fn new(device_name: &str, kind: &str) -> Self {
        let what = if kind == "mic" { "microphone" } else { "sound" };
        let stamp = chrono::Local::now().format("%Y-%m-%d %H-%M-%S");
        let path = folder().join(format!("{} {} {}.wav", safe(device_name), what, stamp));
        Self { path, writer: None, asked: Instant::now(), started: None, samples: 0, channels: 1, rate: 48_000 }
    }

    /// The phone's stream began: open the file with its format.
    pub fn begin(&mut self, rate: u32, channels: u8) -> Result<(), String> {
        std::fs::create_dir_all(folder()).map_err(|e| e.to_string())?;
        let spec = hound::WavSpec {
            channels: channels.max(1) as u16,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        self.writer = Some(hound::WavWriter::create(&self.path, spec).map_err(|e| e.to_string())?);
        self.started = Some(Instant::now());
        self.channels = spec.channels;
        self.rate = rate;
        Ok(())
    }

    /// 16-bit little-endian PCM from the phone.
    pub fn write(&mut self, pcm: &[u8]) {
        let Some(w) = self.writer.as_mut() else { return };
        for c in pcm.chunks_exact(2) {
            if w.write_sample(i16::from_le_bytes([c[0], c[1]])).is_err() {
                return;
            }
            self.samples += 1;
        }
    }

    /// Closes the file; `None` when nothing was recorded.
    pub fn finish(mut self) -> Option<PathBuf> {
        let w = self.writer.take()?;
        w.finalize().ok()?;
        if self.samples == 0 {
            let _ = std::fs::remove_file(&self.path);
            return None;
        }
        Some(self.path)
    }

    pub fn waited(&self) -> std::time::Duration {
        self.asked.elapsed()
    }

    pub fn state(&self, device: &str, kind: &str) -> RecordingState {
        RecordingState {
            device: device.into(),
            kind: kind.into(),
            live: self.started.is_some(),
            seconds: self.samples as f64 / (self.rate as f64 * self.channels as f64).max(1.0),
            path: self.path.to_string_lossy().into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_pcm_into_a_wav_file() {
        let mut r = Recording::new("Pixel 8/../x", "mic");
        assert!(!r.path.to_string_lossy().contains(".."), "device names can't escape the folder");
        r.path = std::env::temp_dir().join(format!("devti-rec-test-{}.wav", std::process::id()));
        r.begin(16_000, 1).unwrap();
        let pcm: Vec<u8> = (0..1600i16).flat_map(|s| s.to_le_bytes()).collect();
        r.write(&pcm);
        let st = r.state("x", "mic");
        assert!((st.seconds - 0.1).abs() < 1e-9 && st.live);
        let path = r.finish().unwrap();
        let reader = hound::WavReader::open(&path).unwrap();
        assert_eq!(reader.spec().sample_rate, 16_000);
        assert_eq!(reader.len(), 1600);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn empty_recordings_leave_no_file() {
        let mut r = Recording::new("Phone", "relay");
        r.path = std::env::temp_dir().join(format!("devti-rec-empty-{}.wav", std::process::id()));
        r.begin(48_000, 2).unwrap();
        let p = r.path.clone();
        assert!(r.finish().is_none());
        assert!(!p.exists());
    }
}
