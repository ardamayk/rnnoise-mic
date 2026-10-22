//! rnnoise-mic: capture the real microphone, run RNNoise, and feed the clean
//! signal into a virtual microphone that every other app can use.
//!
//! * Windows: output goes to VB-Audio "CABLE Input"; apps record from "CABLE Output".
//! * Linux:   a virtual source "RNNoise-Microphone" is created automatically.
//!
//! Without flags it runs as a tray app (no console window). `--cli`, `--meter`
//! and `--list` run in the terminal instead.

// No console window on Windows; command-line mode re-attaches to the parent shell.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod autostart;
#[cfg(not(target_os = "linux"))]
mod cable;
mod dsp;
mod engine;
mod icon;
mod settings;
mod sys;
mod tray;
#[cfg(target_os = "linux")]
mod vmic_linux;

use anyhow::{Result, bail};
use clap::Parser;
use engine::{Engine, Options};
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Parser)]
#[command(version, about = "System-wide RNNoise microphone noise suppression.\n\
    Runs in the system tray by default; use --cli or --meter to run in the terminal.")]
struct Args {
    /// List audio devices and exit
    #[arg(long)]
    list: bool,
    /// Run in the terminal instead of the system tray
    #[arg(long)]
    cli: bool,
    /// Run in the terminal with a live level / voice-activity meter
    #[arg(long)]
    meter: bool,
    /// Real microphone (name substring or device id). Default: system default mic
    #[arg(short, long)]
    input: Option<String>,
    /// Output device (name substring or id). Default: VB-Audio "CABLE Input"
    #[cfg(not(target_os = "linux"))]
    #[arg(short, long)]
    output: Option<String>,
    /// Target buffering between capture and playback, in ms
    #[cfg(not(target_os = "linux"))]
    #[arg(long, default_value_t = 30)]
    latency_ms: u32,
    /// Make RNNoise-Microphone the default input while running
    #[cfg(target_os = "linux")]
    #[arg(long)]
    set_default: bool,
    /// Silence audio when RNNoise's voice probability is below this (0..1, 0 = off)
    #[arg(long, default_value_t = 0.0)]
    vad_gate: f32,
    /// How long the VAD gate stays open after speech, in ms
    #[arg(long, default_value_t = 250)]
    vad_hold_ms: u32,
    /// Start with noise suppression off (pass-through)
    #[arg(long)]
    bypass: bool,
}

impl Args {
    fn options(&self) -> Options {
        Options {
            input: self.input.clone(),
            #[cfg(not(target_os = "linux"))]
            output: self.output.clone(),
            #[cfg(not(target_os = "linux"))]
            latency_ms: self.latency_ms,
            #[cfg(target_os = "linux")]
            set_default: self.set_default,
            vad_gate: self.vad_gate,
            vad_hold_ms: self.vad_hold_ms,
            bypass: self.bypass,
        }
    }
}

fn run_cli(args: &Args) -> Result<()> {
    let Some(_lock) = sys::InstanceLock::acquire() else {
        bail!("RNNoise Mic is already running (check the system tray)");
    };
    let engine = Engine::start(&args.options())?;
    println!("Input : {}", engine.input_name());
    println!("Output: {}", engine.output_name());

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || r.store(false, Ordering::SeqCst))?;
    println!("\nRunning{}. {} Ctrl+C to stop.", if args.bypass { " (bypass)" } else { "" }, engine.hint());

    while running.load(Ordering::SeqCst) {
        if engine.failed() {
            bail!("audio device error (device unplugged?)");
        }
        if args.meter {
            print!("\r{}   ", engine.meter());
            let _ = std::io::stdout().flush();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("\nStopping.");
    Ok(())
}

fn main() -> Result<()> {
    let terminal = std::env::args()
        .skip(1)
        .any(|a| matches!(a.as_str(), "--cli" | "--meter" | "--list" | "-h" | "--help" | "-V" | "--version"));
    if terminal {
        sys::attach_parent_console();
    }
    let args = Args::parse();
    if !(0.0..=1.0).contains(&args.vad_gate) {
        bail!("--vad-gate must be between 0 and 1");
    }
    if args.list {
        engine::list(&engine::host())
    } else if args.cli || args.meter {
        run_cli(&args)
    } else {
        tray::run(args.options())
    }
}
