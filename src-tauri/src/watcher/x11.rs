//! X11: XFixes `SelectSelectionInput` notifies us when the CLIPBOARD owner
//! changes; we then convert TARGETS, the text and the KDE password hint.
//! Also used against XWayland as a best-effort fallback on GNOME/Wayland.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEventMask};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, Property,
    Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;

use super::{ClipboardWatcher, Sink, WatchError, WatcherHandle};
use crate::history::MAX_TEXT_BYTES;
use crate::privacy::{self, ClipboardFormat, ClipboardSnapshot, SourceApp};

/// Upper bound for one selection transfer (owner may hang or stream forever).
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(2);

type Res<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        CLIPBOARD,
        TARGETS,
        UTF8_STRING,
        INCR,
        _NET_ACTIVE_WINDOW,
        PC_SELECTION: b"PASTE_COMMANDER_SELECTION",
        PC_STOP: b"PASTE_COMMANDER_STOP",
        KDE_PASSWORD_HINT: b"x-kde-passwordManagerHint",
    }
}

pub struct X11Watcher {
    name: &'static str,
}

impl X11Watcher {
    pub fn new() -> Self {
        Self { name: "x11-xfixes" }
    }

    /// Same implementation, talking to XWayland inside a Wayland session.
    pub fn xwayland() -> Self {
        Self {
            name: "xwayland-xfixes",
        }
    }
}

impl ClipboardWatcher for X11Watcher {
    fn name(&self) -> &'static str {
        self.name
    }

    fn start(self: Box<Self>, sink: Sink) -> Result<WatcherHandle, WatchError> {
        let (conn, screen) =
            x11rb::connect(None).map_err(|e| WatchError::Unsupported(leak(e.to_string())))?;
        let conn = Arc::new(conn);
        let setup = Session::new(&conn, screen).map_err(|e| WatchError::Failed(e.to_string()))?;
        let (window, stop_atom) = (setup.window, setup.atoms.PC_STOP);

        let thread_conn = conn.clone();
        let thread = std::thread::Builder::new()
            .name("clipboard-watcher".into())
            .spawn(move || {
                if let Err(e) = setup.run(&thread_conn, &sink) {
                    log::error!("X11 clipboard watcher stopped: {e}");
                }
            })
            .map_err(|e| WatchError::Failed(e.to_string()))?;

        Ok(WatcherHandle::new(move || {
            // Wake the blocking event loop with a private client message.
            let event = ClientMessageEvent::new(32, window, stop_atom, [0u32; 5]);
            let _ = conn.send_event(false, window, EventMask::NO_EVENT, event);
            let _ = conn.flush();
            let _ = thread.join();
        }))
    }
}

/// Error messages for `WatchError::Unsupported` must be `'static`; this only
/// happens once per start attempt.
fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

struct Session {
    window: Window,
    root: Window,
    atoms: Atoms,
    atom_names: HashMap<Atom, String>,
}

impl Session {
    fn new(conn: &RustConnection, screen: usize) -> Res<Self> {
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::new(conn)?.reply()?;
        conn.xfixes_query_version(5, 0)?.reply()?;

        let window = conn.generate_id()?;
        conn.create_window(
            0,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_ONLY,
            0,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;
        conn.xfixes_select_selection_input(
            window,
            atoms.CLIPBOARD,
            SelectionEventMask::SET_SELECTION_OWNER
                | SelectionEventMask::SELECTION_WINDOW_DESTROY
                | SelectionEventMask::SELECTION_CLIENT_CLOSE,
        )?;
        conn.flush()?;
        Ok(Self {
            window,
            root,
            atoms,
            atom_names: HashMap::new(),
        })
    }

    fn run(mut self, conn: &RustConnection, sink: &Sink) -> Res<()> {
        loop {
            match conn.wait_for_event()? {
                Event::ClientMessage(ev) if ev.type_ == self.atoms.PC_STOP => return Ok(()),
                Event::XfixesSelectionNotify(ev) if ev.selection == self.atoms.CLIPBOARD => {
                    if ev.owner == x11rb::NONE || ev.owner == self.window {
                        continue;
                    }
                    match self.read_snapshot(conn, ev.selection_timestamp) {
                        Ok(snapshot) => sink(snapshot),
                        Err(e) => log::warn!("reading X11 clipboard failed: {e}"),
                    }
                }
                _ => {}
            }
        }
    }

    fn read_snapshot(&mut self, conn: &RustConnection, time: u32) -> Res<ClipboardSnapshot> {
        let targets: Vec<Atom> = match self.convert(conn, self.atoms.TARGETS, time, 64 * 1024)? {
            Some((_, data)) => data
                .chunks_exact(4)
                .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
            None => Vec::new(),
        };

        let mut formats = Vec::with_capacity(targets.len());
        for &atom in &targets {
            let name = self.atom_name(conn, atom)?;
            let data = if privacy::linux::DATA_FORMATS.contains(&name.as_str()) {
                self.convert(conn, atom, time, 64)?.map(|(_, d)| d)
            } else {
                None
            };
            formats.push(ClipboardFormat { name, data });
        }

        // Do not even transfer the text if it is marked secret.
        let text = if privacy::marker_verdict(&formats).is_some() {
            None
        } else {
            let target = [self.atoms.UTF8_STRING, AtomEnum::STRING.into()]
                .into_iter()
                .find(|t| targets.is_empty() || targets.contains(t));
            match target {
                Some(t) => self
                    .convert(conn, t, time, MAX_TEXT_BYTES + 1)?
                    .map(|(_, d)| String::from_utf8_lossy(&d).into_owned()),
                None => None,
            }
        };

        Ok(ClipboardSnapshot {
            text,
            formats,
            source: self.active_app(conn).ok().flatten(),
        })
    }

    /// Requests `target` from the clipboard owner and reads the reply, handling
    /// the INCR protocol for large transfers. Returns `None` if the owner refuses.
    fn convert(
        &mut self,
        conn: &RustConnection,
        target: Atom,
        time: u32,
        max: usize,
    ) -> Res<Option<(Atom, Vec<u8>)>> {
        let prop = self.atoms.PC_SELECTION;
        conn.convert_selection(self.window, self.atoms.CLIPBOARD, target, prop, time)?;
        conn.flush()?;

        let deadline = Instant::now() + TRANSFER_TIMEOUT;
        // Wait for SelectionNotify addressed to us.
        let notify = loop {
            match self.next_event_until(conn, deadline)? {
                Some(Event::SelectionNotify(ev)) if ev.requestor == self.window => break ev,
                Some(_) => continue,
                None => return Ok(None),
            }
        };
        if notify.property == x11rb::NONE {
            return Ok(None);
        }

        let reply = conn
            .get_property(true, self.window, prop, AtomEnum::ANY, 0, u32::MAX / 4)?
            .reply()?;
        if reply.type_ != self.atoms.INCR {
            let mut data = reply.value;
            data.truncate(max);
            return Ok(Some((reply.type_, data)));
        }

        // INCR: deleting the property (done above) asks the owner for the next
        // chunk; a zero-length chunk ends the transfer.
        let mut data = Vec::new();
        let mut kind = x11rb::NONE;
        loop {
            match self.next_event_until(conn, deadline)? {
                Some(Event::PropertyNotify(ev))
                    if ev.window == self.window
                        && ev.atom == prop
                        && ev.state == Property::NEW_VALUE =>
                {
                    let chunk = conn
                        .get_property(true, self.window, prop, AtomEnum::ANY, 0, u32::MAX / 4)?
                        .reply()?;
                    if chunk.value.is_empty() {
                        return Ok(Some((kind, data)));
                    }
                    kind = chunk.type_;
                    data.extend_from_slice(&chunk.value);
                    if data.len() > max {
                        // Too large to store anyway; keep reading would only waste time.
                        data.truncate(max);
                        return Ok(Some((kind, data)));
                    }
                }
                Some(_) => continue,
                None => return Ok(None),
            }
        }
    }

    /// Polls for events until `deadline`; used only during a transfer so the
    /// idle loop can block without timeouts.
    fn next_event_until(&self, conn: &RustConnection, deadline: Instant) -> Res<Option<Event>> {
        loop {
            if let Some(ev) = conn.poll_for_event()? {
                return Ok(Some(ev));
            }
            if Instant::now() >= deadline {
                return Ok(None);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn atom_name(&mut self, conn: &RustConnection, atom: Atom) -> Res<String> {
        if let Some(n) = self.atom_names.get(&atom) {
            return Ok(n.clone());
        }
        let name = String::from_utf8_lossy(&conn.get_atom_name(atom)?.reply()?.name).into_owned();
        self.atom_names.insert(atom, name.clone());
        Ok(name)
    }

    /// WM_CLASS of `_NET_ACTIVE_WINDOW` (the selection owner is usually a hidden
    /// helper window without a class).
    fn active_app(&self, conn: &RustConnection) -> Res<Option<SourceApp>> {
        let active = conn
            .get_property(
                false,
                self.root,
                self.atoms._NET_ACTIVE_WINDOW,
                AtomEnum::WINDOW,
                0,
                1,
            )?
            .reply()?;
        let Some(window) = active
            .value32()
            .and_then(|mut v| v.next())
            .filter(|w| *w != 0)
        else {
            return Ok(None);
        };
        let class = conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)?
            .reply()?;
        // WM_CLASS = "instance\0Class\0"; the class part is the stable app name.
        let parts: Vec<&[u8]> = class
            .value
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            .collect();
        let Some(class_name) = parts.last() else {
            return Ok(None);
        };
        let id = String::from_utf8_lossy(class_name).into_owned();
        Ok(Some(SourceApp {
            name: Some(id.clone()),
            id,
        }))
    }
}
