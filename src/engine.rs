//! Audio engine: real microphone -> RNNoise pipeline -> virtual microphone.
//! Shared by the tray app and the command-line mode.

#[cfg(not(target_os = "linux"))]
use crate::cable::Output;
use crate::dsp;
#[cfg(target_os = "linux")]
use crate::vmic_linux::Output;

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Host, SampleFormat, Stream, StreamConfig};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[derive(Clone, Default)]
pub struct Options {
    /// Real microphone (name substring or device id); `None` = system default.
    pub input: Option<String>,
    /// Output device (name substring or id); `None` = VB-Audio "CABLE Input".
    #[cfg(not(target_os = "linux"))]
    pub output: Option<String>,
    #[cfg(not(target_os = "linux"))]
    pub latency_ms: u32,
    /// Make RNNoise-Microphone the default input while running.
    #[cfg(target_os = "linux")]
    pub set_default: bool,
    pub vad_gate: f32,
    pub vad_hold_ms: u32,
    pub bypass: bool,
}

pub fn host() -> Host {
    // PulseAudio host (also serves PipeWire via pipewire-pulse) lets us address
    // sources by name; fall back to ALSA when no server is running.
    #[cfg(target_os = "linux")]
    if let Ok(h) = cpal::host_from_id(cpal::HostId::PulseAudio) {
        return h;
    }
    cpal::default_host()
}

pub fn dev_name(d: &Device) -> String {
    d.description().map(|d| d.name().to_owned()).unwrap_or_else(|_| "<unknown>".into())
}

pub fn dev_id(d: &Device) -> String {
    d.id().map(|i| i.id().to_owned()).unwrap_or_default()
}

/// Virtual mics (ours or VB-Cable) must never be the capture source: feedback loop.
fn is_virtual(d: &Device) -> bool {
    let n = dev_name(d).to_lowercase();
    let id = dev_id(d).to_lowercase();
    n.contains("cable output") || n.contains("vb-audio") || n.starts_with("rnnoise") || id.starts_with("rnnoise")
}

/// Xruns, reroutes and refused RT priority are recoverable; losing the device is not.
pub fn is_fatal(e: &cpal::Error) -> bool {
    use cpal::ErrorKind::*;
    matches!(e.kind(), DeviceNotAvailable | StreamInvalidated | HostUnavailable)
}

pub fn find(devs: impl Iterator<Item = Device>, query: &str) -> Option<Device> {
    let q = query.to_lowercase();
    let devs: Vec<_> = devs.collect();
    devs.iter()
        .find(|d| dev_id(d) == query)
        .or_else(|| devs.iter().find(|d| dev_name(d).to_lowercase().contains(&q)))
        .cloned()
}

pub fn list(host: &Host) -> Result<()> {
    println!("Host: {:?}\n\nInputs:", host.id());
    for d in host.input_devices()? {
        println!("  {}   [id: {}]", dev_name(&d), dev_id(&d));
    }
    println!("\nOutputs:");
    for d in host.output_devices()? {
        println!("  {}   [id: {}]", dev_name(&d), dev_id(&d));
    }
    Ok(())
}

fn pick_input(host: &Host, query: Option<&str>) -> Result<Device> {
    if let Some(q) = query {
        return find(host.input_devices()?, q).ok_or_else(|| anyhow!("no input device matches '{q}'"));
    }
    if let Some(d) = host.default_input_device().filter(|d| !is_virtual(d)) {
        return Ok(d);
    }
    // The default mic is probably a virtual one: fall back to the first real one.
    host.input_devices()?
        .find(|d| !is_virtual(d) && !dev_name(d).to_lowercase().contains("monitor"))
        .ok_or_else(|| anyhow!("no physical microphone found"))
}

/// Pick an f32 config, preferring 48 kHz (RNNoise's native rate, no resampling).
pub fn pick_config(dev: &Device, input: bool) -> Result<StreamConfig> {
    let ranges: Vec<_> = if input {
        dev.supported_input_configs()?.collect()
    } else {
        dev.supported_output_configs()?.collect()
    };
    let default = if input { dev.default_input_config()? } else { dev.default_output_config()? };
    let f32_ranges = ranges.iter().filter(|r| r.sample_format() == SampleFormat::F32);
    let best = f32_ranges
        .clone()
        .filter(|r| r.channels() == default.channels())
        .chain(f32_ranges.clone())
        .find_map(|r| r.clone().try_with_sample_rate(dsp::RNNOISE_RATE));
    let cfg = match best {
        Some(c) => c,
        None if default.sample_format() == SampleFormat::F32 => default,
        None => f32_ranges
            .clone()
            .next()
            .map(|r| r.clone().with_max_sample_rate())
            .context("device has no 32-bit float format")?,
    };
    let mut cfg = cfg.config();
    // PulseAudio defaults to ~2 s buffers; ask for 10 ms periods instead.
    if cfg!(target_os = "linux") {
        cfg.buffer_size = cpal::BufferSize::Fixed(cfg.sample_rate / 100);
    }
    Ok(cfg)
}

/// A running engine. Dropping it stops capture, then removes the virtual mic.
pub struct Engine {
    // Field order matters: the capture stream must stop before the output goes away.
    _stream: Stream,
    output: Output,
    input_name: String,
    vad: Arc<AtomicU32>,
    level: Arc<AtomicU32>,
    failed: Arc<AtomicBool>,
    bypass: Arc<AtomicBool>,
}

impl Engine {
    pub fn start(opts: &Options) -> Result<Self> {
        let host = host();
        // Choose the real mic *before* creating the virtual one, so a remembered
        // default of "RNNoise-Microphone" can't make us record ourselves.
        let input = pick_input(&host, opts.input.as_deref())?;
        let in_cfg = pick_config(&input, true)?;

        #[cfg(target_os = "linux")]
        let (output, mut writer) = Output::open(opts.set_default)?;
        #[cfg(not(target_os = "linux"))]
        let (output, mut writer) = Output::open(&host, opts.output.as_deref(), opts.latency_ms)?;

        let gate = dsp::GateConfig { threshold: opts.vad_gate, hold_frames: opts.vad_hold_ms / 10 };
        let mut pipe = dsp::Pipeline::new(in_cfg.sample_rate, in_cfg.channels as usize, output.rate(), gate);
        pipe.bypass.store(opts.bypass, Ordering::Relaxed);
        let bypass = pipe.bypass.clone();

        let vad = Arc::new(AtomicU32::new(0));
        let level = Arc::new(AtomicU32::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        let (vad_w, level_w, fail_w) = (vad.clone(), level.clone(), failed.clone());
        let mut clean = Vec::with_capacity(8192);
        let input_name = format!("{}  ({} Hz, {} ch)", dev_name(&input), in_cfg.sample_rate, in_cfg.channels);
        let stream = input.build_input_stream::<f32, _, _>(
            in_cfg,
            move |data, _| {
                clean.clear();
                pipe.process(data, &mut clean);
                writer.write(&clean);
                vad_w.store(pipe.last_vad.to_bits(), Ordering::Relaxed);
                let peak = data.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                level_w.store(peak.to_bits(), Ordering::Relaxed);
            },
            move |e| {
                if is_fatal(&e) {
                    eprintln!("\ninput stream error: {e}");
                    fail_w.store(true, Ordering::Relaxed);
                }
            },
            None,
        )?;
        stream.play()?;

        Ok(Engine { _stream: stream, output, input_name, vad, level, failed, bypass })
    }

    pub fn input_name(&self) -> &str {
        &self.input_name
    }

    pub fn output_name(&self) -> String {
        self.output.describe()
    }

    pub fn hint(&self) -> &'static str {
        self.output.hint()
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed) || self.output.failed()
    }

    pub fn set_bypass(&self, on: bool) {
        self.bypass.store(on, Ordering::Relaxed);
    }

    /// One-line live status: input level, voice probability, output buffer.
    pub fn meter(&self) -> String {
        let v = f32::from_bits(self.vad.load(Ordering::Relaxed));
        let db = 20.0 * f32::from_bits(self.level.load(Ordering::Relaxed)).max(1e-6).log10();
        format!(
            "in {:>6.1} dBFS | voice {:>3.0}% [{:<20}] | {}",
            db,
            v * 100.0,
            "#".repeat((v * 20.0) as usize),
            self.output.status()
        )
    }
}
