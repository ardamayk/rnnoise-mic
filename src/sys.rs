//! Small platform helpers: console attach, message boxes, opening URLs and a
//! single-instance lock.

/// The release build has no console window on Windows. In command-line mode,
/// reuse the console of the shell that started us so output is visible.
pub fn attach_parent_console() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// Show an informational dialog. Blocks on Windows; fire-and-forget on Linux.
pub fn message(title: &str, text: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MessageBoxW};
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
        let (t, m) = (wide(title), wide(text));
        unsafe {
            MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), MB_OK | MB_ICONINFORMATION | MB_SETFOREGROUND);
        }
    }
    #[cfg(not(windows))]
    {
        use std::process::Command;
        let zenity = Command::new("zenity")
            .args(["--info", "--no-markup", "--width=520", "--title", title, "--text", text])
            .spawn();
        if zenity.is_err() && Command::new("kdialog").args(["--title", title, "--msgbox", text]).spawn().is_err() {
            eprintln!("{title}\n\n{text}");
        }
    }
}

pub fn open_url(url: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
    #[cfg(not(windows))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

/// Held for the lifetime of the process; a second copy fails to acquire it.
pub struct InstanceLock {
    #[cfg(windows)]
    _handle: windows_sys::Win32::Foundation::HANDLE,
    #[cfg(not(windows))]
    _file: std::fs::File,
}

impl InstanceLock {
    /// `None` if another instance is already running.
    pub fn acquire() -> Option<Self> {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
            use windows_sys::Win32::System::Threading::CreateMutexW;
            let name: Vec<u16> = "Local\\rnnoise-mic-single-instance".encode_utf16().chain(Some(0)).collect();
            let handle = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if handle.is_null() || GetLastError() == ERROR_ALREADY_EXISTS {
                return None;
            }
            Some(InstanceLock { _handle: handle })
        }
        #[cfg(not(windows))]
        {
            use std::os::fd::AsRawFd;
            let dir = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(dir.join("rnnoise-mic.lock"))
                .ok()?;
            // Released automatically by the kernel when the process exits, even on a crash.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
                return None;
            }
            Some(InstanceLock { _file: file })
        }
    }
}
