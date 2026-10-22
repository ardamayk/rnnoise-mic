//! "Start at login": HKCU\...\Run on Windows, ~/.config/autostart/*.desktop on Linux.

use anyhow::{Result, anyhow};
use auto_launch::{AutoLaunch, AutoLaunchBuilder};

fn launcher() -> Result<AutoLaunch> {
    let exe = std::env::current_exe()?;
    let exe = exe.to_str().ok_or_else(|| anyhow!("executable path is not valid UTF-8"))?;
    // Re-launch with the same options this copy was started with (e.g. --input).
    let args: Vec<String> = std::env::args().skip(1).collect();
    AutoLaunchBuilder::new()
        .set_app_name("rnnoise-mic")
        .set_app_path(exe)
        .set_args(&args)
        .build()
        .map_err(|e| anyhow!("{e}"))
}

pub fn is_enabled() -> bool {
    launcher().and_then(|l| l.is_enabled().map_err(|e| anyhow!("{e}"))).unwrap_or(false)
}

pub fn set(enabled: bool) -> Result<()> {
    let l = launcher()?;
    if enabled { l.enable() } else { l.disable() }.map_err(|e| anyhow!("{e}"))
}
