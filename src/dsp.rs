//! Audio processing: downmix -> resample to 48 kHz -> RNNoise -> optional VAD gate
//! -> resample to output rate. Runs entirely inside the capture callback.

use nnnoiseless::DenoiseState;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// RNNoise works on 10 ms frames at 48 kHz.
pub const RNNOISE_RATE: u32 = 48_000;
const FRAME: usize = DenoiseState::FRAME_SIZE;
/// RNNoise expects samples in i16 range.
const SCALE: f32 = 32768.0;

/// Streaming linear resampler (good enough for voice, zero latency).
pub struct Resampler {
    step: f64,
    pos: f64,
    prev: f32,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self { step: from as f64 / to as f64, pos: 0.0, prev: 0.0 }
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if input.is_empty() {
            return;
        }
        if self.step == 1.0 {
            out.extend_from_slice(input);
            return;
        }
        // Virtual sequence is [prev, input...]; pos indexes into it.
        let len = input.len() as f64;
        while self.pos < len {
            let i = self.pos as usize;
            let frac = (self.pos - i as f64) as f32;
            let a = if i == 0 { self.prev } else { input[i - 1] };
            let b = input[i];
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        self.pos -= len;
        self.prev = input[input.len() - 1];
    }
}

pub struct GateConfig {
    /// VAD probability below which audio is attenuated. 0 disables the gate.
    pub threshold: f32,
    /// Frames (10 ms each) to keep the gate open after speech ends.
    pub hold_frames: u32,
}

pub struct Pipeline {
    in_channels: usize,
    to_48k: Resampler,
    from_48k: Resampler,
    denoise: Box<DenoiseState<'static>>,
    gate: GateConfig,
    gate_hold: u32,
    gain: f32,
    pending: Vec<f32>,
    frame_in: Vec<f32>,
    frame_out: Vec<f32>,
    mono: Vec<f32>,
    at_48k: Vec<f32>,
    pub last_vad: f32,
    /// When set, audio passes through untouched (for A/B comparison).
    pub bypass: Arc<AtomicBool>,
}

impl Pipeline {
    pub fn new(in_rate: u32, in_channels: usize, out_rate: u32, gate: GateConfig) -> Self {
        Self {
            in_channels,
            to_48k: Resampler::new(in_rate, RNNOISE_RATE),
            from_48k: Resampler::new(RNNOISE_RATE, out_rate),
            denoise: DenoiseState::new(),
            gate,
            gate_hold: 0,
            gain: 1.0,
            pending: Vec::with_capacity(FRAME * 4),
            frame_in: vec![0.0; FRAME],
            frame_out: vec![0.0; FRAME],
            mono: Vec::with_capacity(4096),
            at_48k: Vec::with_capacity(4096),
            last_vad: 0.0,
            bypass: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Feed interleaved capture samples; append mono samples at the output rate to `out`.
    pub fn process(&mut self, interleaved: &[f32], out: &mut Vec<f32>) {
        self.mono.clear();
        let ch = self.in_channels;
        self.mono.extend(
            interleaved
                .chunks_exact(ch)
                .map(|f| f.iter().sum::<f32>() / ch as f32),
        );
        self.at_48k.clear();
        self.to_48k.process(&self.mono, &mut self.at_48k);
        self.pending.extend_from_slice(&self.at_48k);

        let mut done = 0;
        let mut clean = std::mem::take(&mut self.at_48k);
        clean.clear();
        while self.pending.len() - done >= FRAME {
            for (d, s) in self.frame_in.iter_mut().zip(&self.pending[done..done + FRAME]) {
                *d = s * SCALE;
            }
            let target = if self.bypass.load(Ordering::Relaxed) {
                self.frame_out.copy_from_slice(&self.frame_in);
                1.0
            } else {
                let vad = self.denoise.process_frame(&mut self.frame_out, &self.frame_in);
                self.last_vad = vad;
                self.gate_target(vad)
            };
            // Ramp gain across the frame to avoid clicks.
            let step = (target - self.gain) / FRAME as f32;
            for s in &self.frame_out {
                self.gain += step;
                clean.push((s / SCALE * self.gain).clamp(-1.0, 1.0));
            }
            self.gain = target;
            done += FRAME;
        }
        self.pending.drain(..done);
        self.from_48k.process(&clean, out);
        self.at_48k = clean;
    }

    fn gate_target(&mut self, vad: f32) -> f32 {
        if self.gate.threshold <= 0.0 {
            return 1.0;
        }
        if vad >= self.gate.threshold {
            self.gate_hold = self.gate.hold_frames;
            1.0
        } else if self.gate_hold > 0 {
            self.gate_hold -= 1;
            1.0
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_ratio() {
        let mut r = Resampler::new(44_100, 48_000);
        let mut out = Vec::new();
        for _ in 0..100 {
            r.process(&[0.5; 441], &mut out);
        }
        assert!((out.len() as i64 - 48_000).abs() <= 1, "{}", out.len());
        assert!(out[10..].iter().all(|&s| (s - 0.5).abs() < 1e-6));
    }

    #[test]
    fn pipeline_length_is_preserved() {
        let gate = GateConfig { threshold: 0.0, hold_frames: 0 };
        let mut p = Pipeline::new(44_100, 2, 48_000, gate);
        let mut out = Vec::new();
        for _ in 0..100 {
            p.process(&[0.0; 441 * 2], &mut out);
        }
        // One second in -> about one second out (minus <1 frame still pending).
        assert!(out.len() > 48_000 - FRAME && out.len() <= 48_000, "{}", out.len());
    }
}
