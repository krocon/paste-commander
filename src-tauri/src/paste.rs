//! PasteService: puts an entry on the clipboard and, where the platform allows
//! it, returns focus to the previous app and simulates ⌘V / Ctrl+V.
//!
//! Flow (picker): remember previous app when the picker opens → write entry →
//! give focus back → hide picker → short delay → paste keystroke → optionally
//! restore the previous clipboard text.
//! Everything after the clipboard write runs on one worker thread so commands
//! and the UI never block on delays, and the (Wayland) input connection is
//! created once and reused instead of prompting per paste.

use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::error::Result;
use crate::history;
use crate::platform::{DisplayServer, Os, PlatformInfo};
use crate::state::{AppState, HISTORY_CHANGED};
use crate::windows;

/// Time for the target app to become key/active before the keystroke.
const FOCUS_DELAY: Duration = Duration::from_millis(150);
/// Time for the target app to read the clipboard before it is restored.
const RESTORE_DELAY: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// From the picker window: focus must go back to the previous app.
    Picker,
    /// Quick-paste shortcut: the target app still has focus.
    Quick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CopyReason {
    /// macOS without Accessibility permission.
    NoAccessibility,
    /// No way to inject keystrokes in this session.
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PasteOutcome {
    /// Keystroke scheduled.
    Pasted,
    /// Entry is on the clipboard; the user has to press ⌘V / Ctrl+V.
    Copied { reason: CopyReason },
    /// No such entry (e.g. quick paste slot beyond history length).
    Missing,
}

/// Emitted when a scheduled keystroke could not be delivered.
pub const PASTE_FAILED: &str = "paste-failed";

/// Opaque handle of "the app that was active before the picker opened".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusTarget(i64);

struct Job {
    focus: Option<FocusTarget>,
    restore_text: Option<String>,
    entry_text: String,
}

pub struct PasteService {
    tx: mpsc::Sender<Job>,
}

impl PasteService {
    pub fn start<R: Runtime>(app: AppHandle<R>, display: DisplayServer) -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        std::thread::Builder::new()
            .name("paste-worker".into())
            .spawn(move || {
                let mut keys = keys::Keystrokes::new(display);
                for job in rx {
                    run_job(&app, &mut keys, display, job);
                }
            })
            .expect("spawn paste worker");
        Self { tx }
    }
}

/// Whether a keystroke can be attempted right now.
pub fn can_simulate(platform: &PlatformInfo) -> std::result::Result<(), CopyReason> {
    match (platform.os, platform.display_server) {
        (Os::Macos, _) if !accessibility_trusted() => Err(CopyReason::NoAccessibility),
        (Os::Macos | Os::Windows, _) => Ok(()),
        (Os::Linux, DisplayServer::X11 | DisplayServer::Wayland) => Ok(()),
        _ => Err(CopyReason::Unsupported),
    }
}

pub fn paste_entry<R: Runtime>(
    app: &AppHandle<R>,
    id: i64,
    origin: Origin,
) -> Result<PasteOutcome> {
    let state = app.state::<AppState>();
    let Some(entry) = state.history().get(id)? else {
        return Ok(PasteOutcome::Missing);
    };
    paste_text(app, &state, Some(entry.id), entry.content, origin)
}

/// Quick paste: history position 2..=9 without opening the picker.
pub fn quick_paste<R: Runtime>(app: &AppHandle<R>, position: usize) -> Result<PasteOutcome> {
    let state = app.state::<AppState>();
    let Some(entry) = state.history().nth_recent(position)? else {
        return Ok(PasteOutcome::Missing);
    };
    // Quick paste keeps the order stable, so pressing the same shortcut twice
    // pastes the same entry again.
    paste_text(app, &state, None, entry.content, Origin::Quick)
}

fn paste_text<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    touch_id: Option<i64>,
    text: String,
    origin: Origin,
) -> Result<PasteOutcome> {
    let settings = state.settings();
    let simulate = can_simulate(&state.platform());

    // Only restore what the user can see again: never re-publish a clipboard
    // that held concealed (password manager) content.
    let restore_text = (settings.restore_clipboard
        && simulate.is_ok()
        && !state.last_copy_was_private.load(Ordering::Relaxed))
    .then(|| state.clipboard.get_text())
    .flatten()
    .filter(|prev| *prev != text);

    state
        .clipboard
        .set_text(&text, &state.self_writes)
        .map_err(crate::error::Error::Clipboard)?;

    if let Some(id) = touch_id {
        state.history().touch(id, history::now_ms())?;
        app.emit(HISTORY_CHANGED, ())?;
    }

    match simulate {
        Ok(()) => {
            let focus = match origin {
                Origin::Picker => state.take_previous_focus(),
                Origin::Quick => None,
            };
            let _ = state.paste.tx.send(Job {
                focus,
                restore_text,
                entry_text: text,
            });
            Ok(PasteOutcome::Pasted)
        }
        Err(reason) => Ok(PasteOutcome::Copied { reason }),
    }
}

fn run_job<R: Runtime>(
    app: &AppHandle<R>,
    keys: &mut keys::Keystrokes,
    display: DisplayServer,
    job: Job,
) {
    if let Some(target) = job.focus {
        focus::restore(target, display);
    }
    let _ = windows::hide_picker(app);
    std::thread::sleep(FOCUS_DELAY);

    if let Err(e) = keys.send_paste() {
        log::warn!("paste keystroke failed: {e}");
        let _ = app.emit(PASTE_FAILED, e);
        return;
    }

    if let Some(prev) = job.restore_text {
        std::thread::sleep(RESTORE_DELAY);
        let state = app.state::<AppState>();
        // The user may have copied something new meanwhile – don't clobber it.
        if state.clipboard.get_text().as_deref() == Some(job.entry_text.as_str())
            && let Err(e) = state.clipboard.set_text(&prev, &state.self_writes)
        {
            log::warn!("restoring clipboard failed: {e}");
        }
    }
}

// --- accessibility (macOS) ----------------------------------------------------

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> u8;
}

pub fn accessibility_trusted() -> bool {
    #[cfg(target_os = "macos")]
    {
        // SAFETY: argument-less query of this process' TCC status.
        unsafe { AXIsProcessTrusted() != 0 }
    }
    #[cfg(not(target_os = "macos"))]
    true
}

/// Opens System Settings › Privacy & Security › Accessibility.
pub fn open_accessibility_settings() {
    #[cfg(target_os = "macos")]
    {
        let url = "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";
        if let Err(e) = std::process::Command::new("open").arg(url).spawn() {
            log::error!("could not open System Settings: {e}");
        }
    }
}

// --- focus capture / restore --------------------------------------------------

pub mod focus {
    use super::FocusTarget;
    use crate::platform::DisplayServer;

    /// Called right before the picker is shown.
    pub fn capture(display: DisplayServer) -> Option<FocusTarget> {
        let _ = display;
        #[cfg(target_os = "macos")]
        return mac::frontmost_pid().map(|pid| FocusTarget(i64::from(pid)));

        #[cfg(target_os = "windows")]
        return win::foreground().map(FocusTarget);

        #[cfg(target_os = "linux")]
        return match display {
            DisplayServer::X11 => x11::active_window().map(|w| FocusTarget(i64::from(w))),
            // Wayland: the compositor hands focus back when our window hides.
            _ => None,
        };

        #[allow(unreachable_code)]
        None
    }

    pub fn restore(target: FocusTarget, display: DisplayServer) {
        let _ = (target, display);
        #[cfg(target_os = "macos")]
        mac::activate(target.0 as i32);

        #[cfg(target_os = "windows")]
        win::set_foreground(target.0);

        #[cfg(target_os = "linux")]
        if display == DisplayServer::X11
            && let Err(e) = x11::activate(target.0 as u32)
        {
            log::warn!("restoring X11 focus failed: {e}");
        }
    }

    #[cfg(target_os = "macos")]
    mod mac {
        use std::sync::mpsc;
        use std::time::Duration;

        use objc2::MainThreadMarker;
        use objc2::rc::autoreleasepool;
        use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication, NSWorkspace};

        /// Runs `f` on the main thread (directly if already there) with a timeout,
        /// never blocking the main thread on itself.
        fn on_main<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
            if MainThreadMarker::new().is_some() {
                return Some(autoreleasepool(|_| f()));
            }
            let (tx, rx) = mpsc::channel();
            dispatch2::DispatchQueue::main().exec_async(move || {
                let _ = tx.send(autoreleasepool(|_| f()));
            });
            rx.recv_timeout(Duration::from_millis(500)).ok()
        }

        pub fn frontmost_pid() -> Option<i32> {
            let own = std::process::id() as i32;
            on_main(move || {
                NSWorkspace::sharedWorkspace()
                    .frontmostApplication()
                    .map(|a| a.processIdentifier())
                    .filter(|pid| *pid != own)
            })
            .flatten()
        }

        pub fn activate(pid: i32) {
            on_main(move || {
                if let Some(app) =
                    NSRunningApplication::runningApplicationWithProcessIdentifier(pid)
                {
                    app.activateWithOptions(NSApplicationActivationOptions::empty());
                }
            });
        }
    }

    #[cfg(target_os = "windows")]
    mod win {
        use ::windows::Win32::Foundation::HWND;
        use ::windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

        pub fn foreground() -> Option<i64> {
            // SAFETY: plain query.
            let hwnd = unsafe { GetForegroundWindow() };
            (!hwnd.is_invalid()).then_some(hwnd.0 as i64)
        }

        /// Works because we are still the foreground process (the picker has
        /// focus) when this runs – Windows only lets the foreground process
        /// hand foreground to another window.
        pub fn set_foreground(hwnd: i64) {
            // SAFETY: a stale handle just makes the call fail.
            let ok = unsafe { SetForegroundWindow(HWND(hwnd as _)) };
            if !ok.as_bool() {
                log::warn!("SetForegroundWindow failed");
            }
        }
    }

    #[cfg(target_os = "linux")]
    pub(super) mod x11 {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{
            AtomEnum, ClientMessageEvent, ConnectionExt as _, EventMask,
        };

        type Res<T> = Result<T, Box<dyn std::error::Error>>;

        fn atom(conn: &impl Connection, name: &[u8]) -> Res<u32> {
            Ok(conn.intern_atom(false, name)?.reply()?.atom)
        }

        pub fn active_window() -> Option<u32> {
            let run = || -> Res<Option<u32>> {
                let (conn, screen) = x11rb::connect(None)?;
                let root = conn.setup().roots[screen].root;
                let net_active = atom(&conn, b"_NET_ACTIVE_WINDOW")?;
                let reply = conn
                    .get_property(false, root, net_active, AtomEnum::WINDOW, 0, 1)?
                    .reply()?;
                Ok(reply
                    .value32()
                    .and_then(|mut v| v.next())
                    .filter(|w| *w != 0))
            };
            run().ok().flatten()
        }

        /// EWMH activation request; source indication 2 = "pager", which window
        /// managers honor without focus-stealing prevention.
        pub fn activate(window: u32) -> Res<()> {
            let (conn, screen) = x11rb::connect(None)?;
            let root = conn.setup().roots[screen].root;
            let net_active = atom(&conn, b"_NET_ACTIVE_WINDOW")?;
            let event = ClientMessageEvent::new(32, window, net_active, [2, 0, 0, 0, 0]);
            conn.send_event(
                false,
                root,
                EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
                event,
            )?;
            conn.flush()?;
            Ok(())
        }
    }
}

// --- keystroke ----------------------------------------------------------------

mod keys {
    use crate::platform::DisplayServer;

    /// Lazily created input backend, kept for the app's lifetime.
    pub struct Keystrokes {
        #[allow(dead_code)]
        display: DisplayServer,
        #[cfg(not(target_os = "linux"))]
        enigo: Option<enigo::Enigo>,
        #[cfg(target_os = "linux")]
        wayland: Option<enigo::Enigo>,
    }

    impl Keystrokes {
        pub fn new(display: DisplayServer) -> Self {
            Self {
                display,
                #[cfg(not(target_os = "linux"))]
                enigo: None,
                #[cfg(target_os = "linux")]
                wayland: None,
            }
        }

        pub fn send_paste(&mut self) -> Result<(), String> {
            #[cfg(target_os = "macos")]
            return chord(get_enigo(&mut self.enigo)?, enigo::Key::Meta);

            #[cfg(target_os = "windows")]
            return chord(get_enigo(&mut self.enigo)?, enigo::Key::Control);

            #[cfg(target_os = "linux")]
            return match self.display {
                DisplayServer::X11 => super::xtest::ctrl_v(),
                // virtual-keyboard (wlroots) or the RemoteDesktop portal + libei
                // (GNOME, KDE); the portal asks the user once per session.
                _ => chord(get_enigo(&mut self.wayland)?, enigo::Key::Control),
            };

            #[allow(unreachable_code)]
            Err("keystroke simulation not supported on this OS".into())
        }
    }

    fn get_enigo(slot: &mut Option<enigo::Enigo>) -> Result<&mut enigo::Enigo, String> {
        if slot.is_none() {
            let settings = enigo::Settings {
                // We explain the permission ourselves (settings › platform).
                open_prompt_to_get_permissions: false,
                ..Default::default()
            };
            *slot = Some(enigo::Enigo::new(&settings).map_err(|e| e.to_string())?);
        }
        Ok(slot.as_mut().expect("just set"))
    }

    fn chord(enigo: &mut enigo::Enigo, modifier: enigo::Key) -> Result<(), String> {
        use enigo::{Direction, Key, Keyboard};
        enigo
            .key(modifier, Direction::Press)
            .map_err(|e| e.to_string())?;
        let result = enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| e.to_string());
        // Always release the modifier, even if the click failed.
        enigo
            .key(modifier, Direction::Release)
            .map_err(|e| e.to_string())?;
        result
    }
}

/// X11 sessions use XTest directly: enigo's libei backend would ask the
/// RemoteDesktop portal for permission even on X11.
#[cfg(target_os = "linux")]
mod xtest {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{
        ConnectionExt as _, KEY_PRESS_EVENT, KEY_RELEASE_EVENT, Keycode,
    };
    use x11rb::protocol::xtest::ConnectionExt as _;

    const XK_CONTROL_L: u32 = 0xffe3;
    const XK_V: u32 = 0x0076;

    pub fn ctrl_v() -> Result<(), String> {
        run().map_err(|e| e.to_string())
    }

    fn run() -> Result<(), Box<dyn std::error::Error>> {
        let (conn, screen) = x11rb::connect(None)?;
        let root = conn.setup().roots[screen].root;
        let ctrl = keycode(&conn, XK_CONTROL_L)?.ok_or("no keycode for Control_L")?;
        let v = keycode(&conn, XK_V)?.ok_or("no keycode for v")?;
        for (kind, code) in [
            (KEY_PRESS_EVENT, ctrl),
            (KEY_PRESS_EVENT, v),
            (KEY_RELEASE_EVENT, v),
            (KEY_RELEASE_EVENT, ctrl),
        ] {
            conn.xtest_fake_input(kind, code, x11rb::CURRENT_TIME, root, 0, 0, 0)?;
        }
        // Round-trip so the events are processed before the connection closes.
        conn.get_input_focus()?.reply()?;
        Ok(())
    }

    fn keycode(
        conn: &impl Connection,
        keysym: u32,
    ) -> Result<Option<Keycode>, Box<dyn std::error::Error>> {
        let setup = conn.setup();
        let (min, max) = (setup.min_keycode, setup.max_keycode);
        let map = conn.get_keyboard_mapping(min, max - min + 1)?.reply()?;
        let per = map.keysyms_per_keycode as usize;
        Ok(map
            .keysyms
            .chunks(per)
            .position(|syms| syms.contains(&keysym))
            .map(|i| min + i as u8))
    }
}
