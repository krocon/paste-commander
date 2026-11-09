//! Runtime detection of OS, display server and what Paste Commander can do there.
//!
//! Phase 1 decides from the environment only. Later phases refine the Wayland
//! answer by probing the compositor for `ext-data-control-v1`/`wlr-data-control`
//! and the portals for `GlobalShortcuts`/`RemoteDesktop`.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Macos,
    Windows,
    Linux,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayServer {
    /// macOS / Windows.
    Native,
    X11,
    Wayland,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Support {
    Supported,
    /// Works with caveats (extra permission prompt, portal required, …).
    Limited,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Capability {
    pub support: Support,
    /// i18n key the frontend resolves to an explanation, e.g. `platform.note.gnomeNoDataControl`.
    pub note: Option<&'static str>,
}

impl Capability {
    const fn ok() -> Self {
        Self {
            support: Support::Supported,
            note: None,
        }
    }
    const fn limited(note: &'static str) -> Self {
        Self {
            support: Support::Limited,
            note: Some(note),
        }
    }
    const fn no(note: &'static str) -> Self {
        Self {
            support: Support::Unsupported,
            note: Some(note),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    pub os: Os,
    pub arch: &'static str,
    pub display_server: DisplayServer,
    /// `XDG_CURRENT_DESKTOP` on Linux, e.g. `KDE`, `GNOME`, `sway`.
    pub desktop: Option<String>,
    pub clipboard_watch: Capability,
    pub global_shortcuts: Capability,
    pub paste_simulation: Capability,
    /// Watcher implementation actually running, e.g. `wayland-data-control`.
    pub clipboard_backend: Option<&'static str>,
}

impl PlatformInfo {
    /// Replaces the environment-based guess with what actually happened when
    /// the clipboard watcher was started.
    pub fn record_watcher(&mut self, outcome: Result<crate::watcher::Backend, String>) {
        use crate::watcher::Backend;
        match outcome {
            Ok(Backend::Native(name)) => {
                self.clipboard_backend = Some(name);
                self.clipboard_watch = Capability::ok();
            }
            Ok(Backend::XWaylandFallback) => {
                self.clipboard_backend = Some("xwayland-xfixes");
                self.clipboard_watch = Capability::limited("platform.note.xwaylandFallback");
            }
            Err(e) => {
                self.clipboard_backend = None;
                if self.clipboard_watch.support != Support::Unsupported {
                    log::error!("no clipboard watcher: {e}");
                    self.clipboard_watch = Capability::no("platform.note.watcherFailed");
                }
            }
        }
    }

    pub fn detect() -> Self {
        detect_with(current_os(), std::env::consts::ARCH, |k| {
            std::env::var(k).ok()
        })
    }
}

fn current_os() -> Os {
    if cfg!(target_os = "macos") {
        Os::Macos
    } else if cfg!(target_os = "windows") {
        Os::Windows
    } else if cfg!(target_os = "linux") {
        Os::Linux
    } else {
        Os::Other
    }
}

fn detect_with(os: Os, arch: &'static str, env: impl Fn(&str) -> Option<String>) -> PlatformInfo {
    let non_empty = |k: &str| env(k).filter(|v| !v.is_empty());

    let (display_server, desktop) = match os {
        Os::Macos | Os::Windows => (DisplayServer::Native, None),
        Os::Linux | Os::Other => {
            let session = non_empty("XDG_SESSION_TYPE").map(|s| s.to_lowercase());
            let ds = match session.as_deref() {
                Some("wayland") => DisplayServer::Wayland,
                Some("x11") => DisplayServer::X11,
                _ if non_empty("WAYLAND_DISPLAY").is_some() => DisplayServer::Wayland,
                _ if non_empty("DISPLAY").is_some() => DisplayServer::X11,
                _ => DisplayServer::Unknown,
            };
            (ds, non_empty("XDG_CURRENT_DESKTOP"))
        }
    };

    let (clipboard_watch, global_shortcuts, paste_simulation) = match (os, display_server) {
        (Os::Macos, _) => (
            Capability::ok(),
            Capability::ok(),
            Capability::limited("platform.note.macAccessibility"),
        ),
        (Os::Windows, _) => (
            Capability::ok(),
            Capability::ok(),
            Capability::limited("platform.note.windowsUipi"),
        ),
        (_, DisplayServer::X11) => (Capability::ok(), Capability::ok(), Capability::ok()),
        (_, DisplayServer::Wayland) => {
            let is_gnome = desktop
                .as_deref()
                .is_some_and(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("gnome")));
            let watch = if is_gnome {
                Capability::no("platform.note.gnomeNoDataControl")
            } else {
                Capability::limited("platform.note.waylandDataControl")
            };
            (
                watch,
                Capability::limited("platform.note.portalShortcuts"),
                Capability::limited("platform.note.waylandPaste"),
            )
        }
        _ => (
            Capability::no("platform.note.unknownSession"),
            Capability::no("platform.note.unknownSession"),
            Capability::no("platform.note.unknownSession"),
        ),
    };

    PlatformInfo {
        os,
        arch,
        display_server,
        desktop,
        clipboard_watch,
        global_shortcuts,
        paste_simulation,
        clipboard_backend: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn linux(vars: &[(&str, &str)]) -> PlatformInfo {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        detect_with(Os::Linux, "x86_64", |k| map.get(k).cloned())
    }

    #[test]
    fn macos_and_windows_are_native() {
        let mac = detect_with(Os::Macos, "aarch64", |_| None);
        assert_eq!(mac.display_server, DisplayServer::Native);
        assert_eq!(mac.clipboard_watch.support, Support::Supported);
        let win = detect_with(Os::Windows, "x86_64", |_| Some("ignored".into()));
        assert_eq!(win.display_server, DisplayServer::Native);
        assert_eq!(win.desktop, None);
    }

    #[test]
    fn x11_session_is_fully_supported() {
        let p = linux(&[("XDG_SESSION_TYPE", "x11"), ("DISPLAY", ":0")]);
        assert_eq!(p.display_server, DisplayServer::X11);
        assert_eq!(p.clipboard_watch.support, Support::Supported);
        assert_eq!(p.paste_simulation.support, Support::Supported);
    }

    #[test]
    fn gnome_wayland_cannot_watch_clipboard() {
        let p = linux(&[
            ("XDG_SESSION_TYPE", "wayland"),
            ("XDG_CURRENT_DESKTOP", "ubuntu:GNOME"),
        ]);
        assert_eq!(p.display_server, DisplayServer::Wayland);
        assert_eq!(p.clipboard_watch.support, Support::Unsupported);
        assert_eq!(
            p.clipboard_watch.note,
            Some("platform.note.gnomeNoDataControl")
        );
    }

    #[test]
    fn kde_wayland_uses_data_control() {
        let p = linux(&[
            ("WAYLAND_DISPLAY", "wayland-0"),
            ("XDG_CURRENT_DESKTOP", "KDE"),
        ]);
        assert_eq!(p.display_server, DisplayServer::Wayland);
        assert_eq!(p.clipboard_watch.support, Support::Limited);
        assert_eq!(p.desktop.as_deref(), Some("KDE"));
    }

    #[test]
    fn session_type_wins_over_display_vars() {
        // XWayland sets DISPLAY inside a Wayland session.
        let p = linux(&[("XDG_SESSION_TYPE", "wayland"), ("DISPLAY", ":0")]);
        assert_eq!(p.display_server, DisplayServer::Wayland);
    }

    #[test]
    fn watcher_outcome_overrides_guess() {
        use crate::watcher::Backend;
        let mut kde = linux(&[
            ("XDG_SESSION_TYPE", "wayland"),
            ("XDG_CURRENT_DESKTOP", "KDE"),
        ]);
        kde.record_watcher(Ok(Backend::Native("wayland-data-control")));
        assert_eq!(kde.clipboard_watch.support, Support::Supported);
        assert_eq!(kde.clipboard_backend, Some("wayland-data-control"));

        let mut gnome = linux(&[
            ("XDG_SESSION_TYPE", "wayland"),
            ("XDG_CURRENT_DESKTOP", "GNOME"),
        ]);
        gnome.record_watcher(Ok(Backend::XWaylandFallback));
        assert_eq!(gnome.clipboard_watch.support, Support::Limited);

        let mut failed = linux(&[("XDG_SESSION_TYPE", "x11")]);
        failed.record_watcher(Err("boom".into()));
        assert_eq!(failed.clipboard_watch.support, Support::Unsupported);
        assert_eq!(
            failed.clipboard_watch.note,
            Some("platform.note.watcherFailed")
        );
    }

    #[test]
    fn headless_is_unknown() {
        let p = linux(&[("XDG_SESSION_TYPE", "")]);
        assert_eq!(p.display_server, DisplayServer::Unknown);
        assert_eq!(p.global_shortcuts.support, Support::Unsupported);
    }
}
