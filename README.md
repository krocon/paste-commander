# Paste Commander

Clipboard history for **macOS, Windows and Linux**. Everything you copy lands in a local history. A global shortcut opens a small picker, and the chosen entry is pasted straight into the app you were working in. Quick-paste shortcuts insert the 2nd–9th most recent entry without opening a window.

Built with **Tauri 2 + Rust**; the UI is **Angular 22** (zoneless, signals, CSS variables). All logic lives in Rust. Everything stays local, and the app makes no network connections.

## Features

- **Background recording** of copied text. A text copied again moves to the top instead of being added twice. Blank text is ignored, and so are Paste Commander's own clipboard writes.
- **Picker:**
  - Opens on a global shortcut, centered on the monitor under the mouse. On macOS it also appears over full-screen apps and on every Space.
  - Instant search over the full text.
  - Pin, delete and keyboard-only operation.
- **Direct paste into the previous app:** focus goes back to that app and ⌘V / Ctrl+V is simulated. Optionally, the previous clipboard text is restored afterwards.
- **Quick paste:** shortcuts paste entries 2–9 without a window.
- **Privacy:**
  - Content marked confidential by password managers is never stored.
  - An app exclusion list (pre-filled with common password managers).
  - Pause mode.
  - Everything stays local.
- **Settings:**
  - Shortcut recorder.
  - History size (10–500).
  - Persistence and "clear history on quit".
  - Clipboard restore and launch at login.
  - Platform status page showing what works in this session and which permissions are missing.
- **Languages:** German and English, plus light and dark mode.

## Platform matrix

Verified November 2026.

| Capability | macOS | Windows | Linux X11 | Wayland: KDE, Sway, Hyprland, … | Wayland: GNOME |
|---|---|---|---|---|---|
| Detect copies in the background | ✅ `NSPasteboard.changeCount` | ✅ `AddClipboardFormatListener` | ✅ XFixes | ✅ `ext-data-control-v1` (KWin 6.7+, Sway 1.11+, Hyprland 0.52+), fallback `wlr-data-control` | ❌ Mutter implements neither protocol. Best effort via XWayland only |
| Global shortcuts | ✅ | ✅ | ✅ | ⚠️ XDG `GlobalShortcuts` portal, where the desktop confirms the binding | ⚠️ portal (GNOME 48+) |
| Paste into the active app | ✅ with Accessibility permission | ✅ `SendInput` | ✅ XTest | ⚠️ wlroots: virtual keyboard. KDE: `RemoteDesktop` portal + libei | ⚠️ `RemoteDesktop` portal + libei (asks for permission) |
| Fallback | Copy and show the hint "press ⌘V" | – | – | Copy, you press Ctrl+V | Copy, you press Ctrl+V |

How the app adapts at runtime:
- It detects the session from `XDG_SESSION_TYPE`, `WAYLAND_DISPLAY` and `XDG_CURRENT_DESKTOP`.
- It starts the best available watcher.
- **Settings › Platform** shows what is actually active and what is not supported.

## Shortcuts

| | macOS | Windows / Linux |
|---|---|---|
| Open picker | ⇧⌘V | Ctrl+Alt+V |
| Paste entry 2 / 3 (slots 4–9 can be assigned) | ⌃⌥2 / ⌃⌥3 | Ctrl+Alt+Shift+2 / 3 |

Why these defaults:
- Windows reports AltGr as Ctrl+Alt, so Ctrl+Alt+2/3 would swallow `²`/`³` on German keyboards.
- Ctrl+Shift+V is "paste" in most Linux terminals.
- ⇧⌘V shadows "Paste and Match Style" in some macOS apps. If you need that, record e.g. ⌥⌘V.

Picker keys:

| Key | Action |
|---|---|
| ↑ / ↓, PgUp / PgDn | Select |
| ⏎ (also ⇧⏎) | Paste the selected entry |
| ⌘1…⌘9 / Ctrl+1…Ctrl+9 | Paste entry 1–9 directly |
| ⌘P / Ctrl+P | Pin / unpin |
| Del | Delete the entry |
| Esc, or the window losing focus | Close |

Pasting from the picker moves the entry to the top. Quick paste keeps the order, so pressing the same shortcut twice pastes the same entry.

## Permissions

- **macOS:**
  - **Accessibility** (System Settings › Privacy & Security › Accessibility) is needed to simulate ⌘V.
  - On first launch without it, the settings window opens on the Platform page with a button to the right pane.
  - Without the permission, entries are copied and a hint says "press ⌘V".
- **Windows:** no permissions. Pasting into elevated (administrator) windows is blocked by UIPI unless Paste Commander runs elevated too.
- **Linux X11:** none.
- **Linux Wayland:**
  - **Shortcuts:** the desktop shows a dialog to confirm the global shortcuts.
  - **Pasting:** GNOME and KDE ask once per session for "remote desktop / input" permission. Sway and Hyprland use the virtual keyboard without a dialog.

## Install

Releases on GitHub provide:
- macOS: a universal `.dmg`.
- Windows: `.msi` and NSIS setup `.exe`.
- Linux: `.AppImage`, `.deb` and `.rpm`.

Linux runtime dependencies: WebKitGTK 4.1 and libayatana-appindicator (declared by the .deb).

## Development

Prerequisites:
- Rust stable (≥ 1.85, edition 2024)
- Node 24
- On Linux, the packages below

```bash
npm install
```

```bash
npm run tauri dev
```

The Angular dev server runs on port 1420. For pure UI work in a browser: `npm start`, then open `http://localhost:1420/` (settings) or `http://localhost:1420/?view=picker`.

Tests:

```bash
npm test
```

```bash
cd src-tauri && cargo test && cargo clippy --all-targets -- -D warnings
```

```bash
scripts/linux-watcher-tests.sh
```

The last one is Linux only. It runs the real watchers against Xvfb and a headless sway and needs `xvfb xclip sway`.

Performance check (500 entries: list ≈ 1 ms, search ≈ 2 ms on an M-series Mac):

```bash
cd src-tauri && cargo test --release -- --ignored perf_ --nocapture
```

### Linux build dependencies

Debian/Ubuntu:

```bash
sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev build-essential
```

Fedora: `webkit2gtk4.1-devel libappindicator-gtk3-devel librsvg2-devel libxdo-devel openssl-devel`.

## Release and signing

Push a tag `vX.Y.Z` that matches `version` in `src-tauri/tauri.conf.json`. [`release.yml`](.github/workflows/release.yml) then builds all installers and attaches them to a **draft** release. Signing is optional: without secrets, the builds are unsigned.

**macOS (Developer ID + notarization)**

1. Export the Developer ID Application certificate as `.p12`.
2. Set these repository secrets:
   - `APPLE_CERTIFICATE`: the `.p12` file, base64-encoded.
   - `APPLE_CERTIFICATE_PASSWORD`.
   - `APPLE_SIGNING_IDENTITY`: e.g. `Developer ID Application: Name (TEAMID)`.
   - `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID`.

Tauri signs with the hardened runtime and notarizes automatically. `LSUIElement` keeps the app out of the Dock.

**Windows: options**

| Option | Cost (2026) | Fit |
|---|---|---|
| **Azure Artifact Signing** (formerly Trusted Signing) | approx. $9.99/month | Best SmartScreen reputation path. In Germany it is available to **organizations only**; individuals must be in the US or Canada. The workflow supports it. |
| OV/EV certificate (e.g. Certum, Sectigo) on a hardware token or cloud HSM | approx. €70–400/year | Works for individuals. Sign via `bundle.windows.signCommand` (e.g. `signtool` or a vendor cloud CLI). |
| SignPath Foundation | free for OSS | For open-source projects after approval. Signing runs in their CI integration. |
| Unsigned | – | SmartScreen warns ("unknown publisher") until downloads build reputation. |

For Azure Artifact Signing, set:
- secrets `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` and `AZURE_TENANT_ID`
- repository variables `AZURE_SIGNING_ENDPOINT`, `AZURE_SIGNING_ACCOUNT` and `AZURE_SIGNING_PROFILE`

**Linux:** AppImage, .deb and .rpm are not signed. Publish checksums with the release if needed.

## Project layout

```
src/                      Angular UI (one bundle, the window label picks the view)
  app/core/               Backend facade (signals), i18n (de/en), accelerator helpers
  app/picker/             Picker: list, search, keyboard, relative time
  app/settings/           Settings window, shortcut recorder, platform status
src-tauri/src/
  lib.rs                  App setup, plugins, run loop, exit handling
  settings.rs             Settings model, validation, persistence (tauri-plugin-store)
  platform.rs             OS/session/capability detection, permissions
  history/                SQLite history: dedupe, pins, limit, search, persistence switch
  privacy.rs              Confidential markers (macOS/Windows/KDE), app exclusion list
  ingest.rs               watcher → pause → own-write guard → privacy → history
  watcher/                `ClipboardWatcher` trait and the runtime choice of watcher
    macos.rs              NSPasteboard changeCount polling (250 ms)
    windows.rs            AddClipboardFormatListener (message-only window)
    x11.rs                XFixes selection events, TARGETS/INCR; also the XWayland fallback
    wayland.rs            ext/wlr data-control via wl-clipboard-rs
    linux_it.rs           Integration tests against Xvfb and a headless sway
  clipboard.rs            Clipboard writes (arboard), registered with the own-write guard
  paste.rs                PasteService: focus restore, keystroke (enigo/XTest), clipboard restore
  hotkeys.rs              Global shortcuts: Tauri plugin, or the XDG portal on Wayland
  tray.rs, windows.rs     Tray menu; picker (pre-created) and settings windows
  commands.rs             Tauri commands (allow-listed in build.rs + capabilities/)
  i18n.rs                 Native strings (tray, notifications)
```

## Storage and privacy

- **Location:** `history.sqlite3` in the app data directory:
  - macOS: `~/Library/Application Support/io.github.krocon.pastecommander/`
  - Windows: `%APPDATA%\io.github.krocon.pastecommander\`
  - Linux: `~/.local/share/io.github.krocon.pastecommander/`
- **Database settings:** WAL mode and `secure_delete`. The file is owner-only (0600).
- **Size limits:** texts above 2 MiB are not stored. The history limit (10–500) counts unpinned entries only.
- **"Keep history across restarts" off:** the history lives in memory and the database file is deleted.
- **Clearing:** "Clear history" (tray) and "clear history on quit" keep pinned entries.
- **Confidential content is never stored:**
  - macOS: `org.nspasteboard.{Concealed,Transient,AutoGenerated}Type`
  - Windows: `ExcludeClipboardContentFromMonitorProcessing`, or `CanIncludeInClipboardHistory`/`CanUploadToCloudClipboard` = 0
  - Linux: `x-kde-passwordManagerHint` = `secret`. On Linux the secret text is not even transferred.
- **Clipboard restore:** the previous clipboard is not restored if it held such content.

## Security

- Every command must be listed in `build.rs` and granted in `src-tauri/capabilities/default.json`.
- Strict CSP with no remote sources. `connect-src` only allows Tauri IPC.
- `macOSPrivateApi` is enabled for the transparent picker. That is fine for direct distribution but not allowed in the Mac App Store.

## Known limitations

- **GNOME/Wayland:** copies made in other apps are invisible to third-party apps. The XWayland fallback only sees copies that reach X11 (usually while an X11 app has focus). A GNOME Shell extension bridge is a phase-2 option.
- **Wayland source app:** the protocol does not reveal which app copied, so "excluded apps" cannot apply there. Confidential markers still work.
- **macOS Secure Input:** while a password field or an app with Secure Input is active, simulated ⌘V is ignored.
- **Windows UIPI:** you cannot paste into elevated windows from a non-elevated Paste Commander.
- **Synthetic keystrokes:** some apps (games, remote-desktop and VM clients, some terminals) ignore them. In terminals with Ctrl+Shift+V paste, Ctrl+V may do nothing.
- **Plain text only:** clipboard restore only restores text. Images and files arrive in phase 2.

## License

[MIT](LICENSE)
