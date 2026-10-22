//! System tray app: runs the engine in the background with no window.
//! Menu: status, noise suppression on/off, start at login, about, quit.

use crate::engine::{Engine, Options};
use crate::icon::{self, State};
use crate::{autostart, sys};
use anyhow::Result;
use std::time::{Duration, Instant};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

pub const REPO_URL: &str = "https://github.com/ardamayk/rnnoise-mic";
/// How often to check the engine and retry after a failure (e.g. mic unplugged).
const TICK: Duration = Duration::from_secs(2);

fn tray_icon(state: State) -> Option<Icon> {
    let (rgba, w, h) = icon::rgba(state);
    Icon::from_rgba(rgba, w, h).ok()
}

fn about_text(engine: Option<&Engine>) -> String {
    #[cfg(windows)]
    let how = "\
How it works
Your real microphone -> RNNoise -> VB-Audio Virtual Cable.
Apps then use \"CABLE Output\" as their microphone.

Setup (Windows)
1. Install VB-Audio Virtual Cable (vb-audio.com/Cable) and reboot.
2. Settings > System > Sound > Input: choose \"CABLE Output\".
   Keep Output on your headphones/speakers - never \"CABLE Input\".
3. Apps that use a specific mic (Discord, OBS, Zoom...): select
   \"CABLE Output\" or \"Default\" in their audio settings.

While RNNoise Mic is not running, CABLE Output is silent -
turn on \"Start at login\" to keep it always available.";
    #[cfg(not(windows))]
    let how = "\
How it works
Your real microphone -> RNNoise -> virtual microphone
\"RNNoise-Microphone\", created automatically while the app runs.

Setup (Linux)
1. Needs PulseAudio or PipeWire (pipewire-pulse) and `pactl`.
2. In Settings > Sound > Input, or in each app (Discord, OBS,
   browser...), choose \"RNNoise-Microphone\".
3. The virtual microphone disappears when the app quits -
   turn on \"Start at login\" to keep it always available.";

    let devices = match engine {
        Some(e) => format!("Input:  {}\nOutput: {}", e.input_name(), e.output_name()),
        None => "Engine is not running.".to_owned(),
    };
    format!(
        "RNNoise Mic {}\nSystem-wide microphone noise suppression (RNNoise).\n\n{how}\n\n\
         Tip: turn off other noise suppression (Krisp etc.) in apps.\n\n{devices}\n\n\
         MIT License - {REPO_URL}",
        env!("CARGO_PKG_VERSION")
    )
}

enum UserEvent {
    Menu(MenuEvent),
}

pub fn run(opts: Options) -> Result<()> {
    let Some(_lock) = sys::InstanceLock::acquire() else {
        sys::message("RNNoise Mic", "RNNoise Mic is already running (see the system tray).");
        return Ok(());
    };

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::Menu(e));
    }));

    let status = MenuItem::new("Starting...", false, None);
    let suppress = CheckMenuItem::new("Noise suppression", true, !opts.bypass, None);
    let login = CheckMenuItem::new("Start at login", true, autostart::is_enabled(), None);
    let about = MenuItem::new("About / How to use", true, None);
    let website = MenuItem::new("Project page", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append_items(&[
        &MenuItem::new("RNNoise Mic", false, None),
        &status,
        &PredefinedMenuItem::separator(),
        &suppress,
        &login,
        &PredefinedMenuItem::separator(),
        &about,
        &website,
        &PredefinedMenuItem::separator(),
        &quit,
    ])?;

    let mut opts = opts;
    let mut engine: Option<Engine> = None;
    let mut tray: Option<TrayIcon> = None;
    let mut shown: Option<(State, String)> = None;
    let mut last_try: Option<Instant> = None;
    let mut error: Option<String> = None;
    let mut menu = Some(menu);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + TICK);
        match event {
            // The tray must be created once the loop runs (GTK / macOS requirement).
            Event::NewEvents(StartCause::Init) => {
                tray = menu.take().and_then(|m| {
                    TrayIconBuilder::new()
                        .with_menu(Box::new(m))
                        .with_tooltip("RNNoise Mic")
                        .with_icon(tray_icon(State::Active)?)
                        .build()
                        .ok()
                });
            }
            Event::UserEvent(UserEvent::Menu(e)) => {
                if e.id == quit.id() {
                    engine = None; // stop audio and remove the virtual mic before exiting
                    tray = None;
                    *control_flow = ControlFlow::Exit;
                    return;
                } else if e.id == suppress.id() {
                    opts.bypass = !suppress.is_checked();
                    if let Some(en) = &engine {
                        en.set_bypass(opts.bypass);
                    }
                } else if e.id == login.id() {
                    let want = login.is_checked();
                    if let Err(err) = autostart::set(want) {
                        login.set_checked(!want);
                        sys::message("RNNoise Mic", &format!("Could not change start-at-login:\n{err}"));
                    }
                } else if e.id == about.id() {
                    sys::message("About RNNoise Mic", &about_text(engine.as_ref()));
                } else if e.id == website.id() {
                    sys::open_url(REPO_URL);
                }
            }
            _ => {}
        }

        // Keep the engine alive: (re)start it when missing or failed.
        if engine.as_ref().is_some_and(|e| e.failed()) {
            engine = None;
        }
        if engine.is_none() && last_try.is_none_or(|t| t.elapsed() >= TICK) {
            last_try = Some(Instant::now());
            match Engine::start(&opts) {
                Ok(e) => {
                    engine = Some(e);
                    error = None;
                }
                Err(e) => error = Some(e.to_string()),
            }
        }

        let (state, text) = match (&engine, error.clone()) {
            (Some(e), _) => {
                let name = e.input_name().split("  (").next().unwrap_or_default().to_owned();
                if opts.bypass {
                    (State::Bypassed, format!("Bypassed: {name}"))
                } else {
                    (State::Active, format!("Active: {name}"))
                }
            }
            (None, Some(err)) => (State::Error, format!("Error: {err} (retrying)")),
            (None, None) => (State::Error, "Stopped".to_owned()),
        };
        if shown.as_ref() != Some(&(state, text.clone())) {
            status.set_text(&text);
            if let Some(t) = &tray {
                let _ = t.set_icon(tray_icon(state));
                let _ = t.set_tooltip(Some(format!("RNNoise Mic - {text}")));
            }
            shown = Some((state, text));
        }
    })
}
