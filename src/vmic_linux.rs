//! Linux virtual microphone: a PulseAudio / pipewire-pulse `module-pipe-source`
//! fed through a FIFO. Apps pick "RNNoise-Microphone" like any other mic.
//!
//! The source is clocked by the data we write, so there is no second device clock
//! to drift against and no playback buffer to manage. The module is removed on drop.

use anyhow::{Context, Result, bail};
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub const SOURCE: &str = "rnnoise_mic";
pub const RATE: u32 = crate::dsp::RNNOISE_RATE;
/// Writes up to PIPE_BUF bytes to a pipe are atomic, so a non-blocking write
/// never splits a sample.
const PIPE_BUF: usize = 4096;

fn pactl(args: &[&str]) -> Result<String> {
    let out = Command::new("pactl")
        .args(args)
        .output()
        .context("`pactl` not found; install pulseaudio-utils (works with PipeWire too)")?;
    if !out.status.success() {
        bail!("pactl {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

pub fn default_source() -> Option<String> {
    pactl(&["get-default-source"]).ok().filter(|s| !s.is_empty())
}

/// Remove a module left behind by a previous run that crashed.
fn cleanup_stale() {
    let Ok(list) = pactl(&["list", "short", "modules"]) else { return };
    for line in list.lines() {
        if line.contains(&format!("source_name={SOURCE}")) {
            if let Some(id) = line.split_whitespace().next() {
                let _ = pactl(&["unload-module", id]);
            }
        }
    }
}

fn fifo_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    dir.join("rnnoise-mic.fifo")
}

pub struct Output {
    module: String,
    fifo: PathBuf,
    previous_default: Option<String>,
    dropped: Arc<AtomicU64>,
}

/// Real-time safe writer handed to the capture callback.
pub struct Writer {
    file: File,
    bytes: Vec<u8>,
    dropped: Arc<AtomicU64>,
}

impl Writer {
    pub fn write(&mut self, samples: &[f32]) {
        for chunk in samples.chunks(PIPE_BUF / 4) {
            self.bytes.clear();
            self.bytes.extend(chunk.iter().flat_map(|s| s.to_le_bytes()));
            match self.file.write(&self.bytes) {
                Ok(_) => {}
                // Nobody is draining the source right now: drop instead of blocking.
                Err(e) if e.kind() == ErrorKind::WouldBlock => {
                    self.dropped.fetch_add(chunk.len() as u64, Ordering::Relaxed);
                }
                Err(_) => {}
            }
        }
    }
}

impl Output {
    pub fn open(make_default: bool) -> Result<(Self, Writer)> {
        cleanup_stale();
        let fifo = fifo_path();
        let _ = std::fs::remove_file(&fifo);
        let module = pactl(&[
            "load-module",
            "module-pipe-source",
            &format!("source_name={SOURCE}"),
            &format!("file={}", fifo.display()),
            "format=float32le",
            &format!("rate={RATE}"),
            "channels=1",
            "source_properties=device.description=RNNoise-Microphone",
        ])?;
        let dropped = Arc::new(AtomicU64::new(0));
        let mut out = Output { module, fifo, previous_default: None, dropped: dropped.clone() };

        // The module created the FIFO and holds it open for reading, so this
        // non-blocking open succeeds immediately.
        let file = OpenOptions::new()
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(&out.fifo)
            .with_context(|| format!("opening {}", out.fifo.display()))?;

        if make_default {
            out.previous_default = default_source().filter(|s| s != SOURCE);
            pactl(&["set-default-source", SOURCE])?;
        }
        Ok((out, Writer { file, bytes: Vec::with_capacity(PIPE_BUF), dropped }))
    }

    pub fn rate(&self) -> u32 {
        RATE
    }

    pub fn describe(&self) -> String {
        format!("RNNoise-Microphone (virtual source '{SOURCE}', {RATE} Hz, 1 ch)")
    }

    pub fn status(&self) -> String {
        let ms = self.dropped.load(Ordering::Relaxed) * 1000 / RATE as u64;
        format!("dropped {ms} ms")
    }

    pub fn failed(&self) -> bool {
        false
    }

    pub fn hint(&self) -> &'static str {
        "In your apps choose the microphone \"RNNoise-Microphone\"."
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        if let Some(prev) = &self.previous_default {
            let _ = pactl(&["set-default-source", prev]);
        }
        let _ = pactl(&["unload-module", &self.module]);
        let _ = std::fs::remove_file(&self.fifo);
    }
}
