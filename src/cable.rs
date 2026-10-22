//! Windows virtual microphone: play the clean signal into a virtual audio cable
//! (VB-Audio "CABLE Input"); apps record from its other end ("CABLE Output").
//!
//! Capture and playback run on different device clocks, so a ring buffer sits
//! between them and the playback side trims it when it grows too large.

use crate::engine::{dev_name, find, is_fatal, pick_config};
use anyhow::{Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Host, Stream};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapProd, HeapRb};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub struct Output {
    _stream: Stream,
    name: String,
    rate: u32,
    channels: u16,
    fill_ms: Arc<AtomicU32>,
    underruns: Arc<AtomicU64>,
    failed: Arc<AtomicBool>,
}

/// Real-time safe writer handed to the capture callback.
pub struct Writer(HeapProd<f32>);

impl Writer {
    pub fn write(&mut self, samples: &[f32]) {
        self.0.push_slice(samples); // drops the excess if the ring is full
    }
}

/// The playback end of VB-Cable. Windows localizes the endpoint name
/// ("CABLE Input", "Hoparlör (VB-Audio Virtual Cable)", ...), so match on the
/// driver name and skip the 16-channel variant.
fn find_cable(host: &Host) -> Result<Option<cpal::Device>> {
    let devs: Vec<_> = host.output_devices()?.collect();
    let name = |d: &cpal::Device| dev_name(d).to_lowercase();
    Ok(devs
        .iter()
        .find(|d| name(d).contains("cable input"))
        .or_else(|| {
            devs.iter().find(|d| {
                let n = name(d);
                n.contains("vb-audio virtual cable") && !n.contains("16")
            })
        })
        .cloned())
}

impl Output {
    pub fn open(host: &Host, query: Option<&str>, latency_ms: u32) -> Result<(Self, Writer)> {
        let device = match query {
            Some(q) => find(host.output_devices()?, q).ok_or_else(|| anyhow!("no output device matches '{q}'"))?,
            None => find_cable(host)?.ok_or_else(|| {
                anyhow!(
                    "VB-Audio Virtual Cable not found.\n  Install it from https://vb-audio.com/Cable/ \
                     (free), reboot, then run again.\n  Or pass --output <device> to use another virtual cable."
                )
            })?,
        };
        let cfg = pick_config(&device, false)?;
        let rate = cfg.sample_rate;
        let channels = cfg.channels;

        let rate_us = rate as usize;
        let target = rate_us * latency_ms as usize / 1000;
        let max_fill = target * 4 + rate_us / 50;
        let (prod, mut cons) = HeapRb::<f32>::new(rate_us).split();

        let fill_ms = Arc::new(AtomicU32::new(0));
        let underruns = Arc::new(AtomicU64::new(0));
        let failed = Arc::new(AtomicBool::new(false));
        let (fill_w, under_w, fail_w) = (fill_ms.clone(), underruns.clone(), failed.clone());

        let ch = channels as usize;
        let mut primed = false;
        let mut mono = Vec::<f32>::new();
        let stream = device.build_output_stream::<f32, _, _>(
            cfg,
            move |data, _| {
                let frames = data.len() / ch;
                let fill = cons.occupied_len();
                fill_w.store((fill * 1000 / rate_us) as u32, Ordering::Relaxed);
                if fill > max_fill {
                    // Capture clock runs faster than playback: drop the excess.
                    cons.skip(fill - target);
                }
                if !primed {
                    if cons.occupied_len() < target.max(frames) {
                        data.fill(0.0);
                        return;
                    }
                    primed = true;
                }
                mono.resize(frames, 0.0);
                let got = cons.pop_slice(&mut mono);
                if got < frames {
                    mono[got..].fill(0.0);
                    primed = false;
                    under_w.fetch_add(1, Ordering::Relaxed);
                }
                for (frame, &s) in data.chunks_exact_mut(ch).zip(&mono) {
                    frame.fill(s);
                }
            },
            move |e| {
                if is_fatal(&e) {
                    eprintln!("\noutput stream error: {e}");
                    fail_w.store(true, Ordering::Relaxed);
                }
            },
            None,
        )?;
        stream.play()?;

        let out = Output { _stream: stream, name: dev_name(&device), rate, channels, fill_ms, underruns, failed };
        Ok((out, Writer(prod)))
    }

    pub fn rate(&self) -> u32 {
        self.rate
    }

    pub fn describe(&self) -> String {
        format!("{}  ({} Hz, {} ch)", self.name, self.rate, self.channels)
    }

    pub fn status(&self) -> String {
        format!(
            "buffer {:>3} ms | underruns {}",
            self.fill_ms.load(Ordering::Relaxed),
            self.underruns.load(Ordering::Relaxed)
        )
    }

    pub fn failed(&self) -> bool {
        self.failed.load(Ordering::Relaxed)
    }

    pub fn hint(&self) -> &'static str {
        "In your apps (or Windows Sound settings > Input) choose \"CABLE Output\" as the microphone."
    }
}
