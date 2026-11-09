//! Windows: a message-only window registered with `AddClipboardFormatListener`
//! receives `WM_CLIPBOARDUPDATE` for every change. No polling.

use std::cell::RefCell;
use std::sync::mpsc;
use std::time::Duration;

use ::windows::Win32::Foundation::{
    CloseHandle, HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM,
};
use ::windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardFormatNameW, GetClipboardOwner, OpenClipboard, RemoveClipboardFormatListener,
};
use ::windows::Win32::System::LibraryLoader::GetModuleHandleW;
use ::windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use ::windows::Win32::System::Ole::CF_UNICODETEXT;
use ::windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow,
    GetMessageW, GetWindowThreadProcessId, HWND_MESSAGE, MSG, PostMessageW, PostQuitMessage,
    RegisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE, WM_CLOSE, WM_DESTROY,
    WNDCLASSW,
};
use ::windows::core::{PWSTR, w};

use super::{ClipboardWatcher, Sink, WatchError, WatcherHandle};
use crate::privacy::{self, ClipboardFormat, ClipboardSnapshot, SourceApp};

thread_local! {
    static SINK: RefCell<Option<Sink>> = const { RefCell::new(None) };
}

pub struct WindowsWatcher;

impl ClipboardWatcher for WindowsWatcher {
    fn name(&self) -> &'static str {
        "windows-format-listener"
    }

    fn start(self: Box<Self>, sink: Sink) -> Result<WatcherHandle, WatchError> {
        // HWND is not Send; pass it across threads as an integer.
        let (ready_tx, ready_rx) = mpsc::channel::<Result<isize, String>>();
        let thread = std::thread::Builder::new()
            .name("clipboard-watcher".into())
            .spawn(move || {
                SINK.with(|s| *s.borrow_mut() = Some(sink));
                // SAFETY: plain Win32 calls on this thread; the window lives on it.
                let hwnd = match unsafe { create_listener_window() } {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(hwnd.0 as isize));
                let mut msg = MSG::default();
                // SAFETY: standard message loop for the window created above.
                unsafe {
                    while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                        DispatchMessageW(&msg);
                    }
                }
            })
            .map_err(|e| WatchError::Failed(e.to_string()))?;

        let hwnd = ready_rx
            .recv()
            .map_err(|e| WatchError::Failed(e.to_string()))?
            .map_err(WatchError::Failed)?;

        Ok(WatcherHandle::new(move || {
            // SAFETY: the window belongs to the watcher thread; WM_CLOSE makes it
            // destroy itself there and end the message loop.
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as _)), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            let _ = thread.join();
        }))
    }
}

unsafe fn create_listener_window() -> Result<HWND, String> {
    unsafe {
        let module = GetModuleHandleW(None).map_err(|e| e.to_string())?;
        let instance = HINSTANCE(module.0);
        let class_name = w!("PasteCommanderClipboardWatcher");
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: class_name,
            ..Default::default()
        };
        // Returns 0 if the class already exists (watcher restarted) – that's fine.
        RegisterClassW(&class);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!(""),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(instance),
            None,
        )
        .map_err(|e| e.to_string())?;
        AddClipboardFormatListener(hwnd).map_err(|e| e.to_string())?;
        Ok(hwnd)
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CLIPBOARDUPDATE => {
            if let Some(snapshot) = read_snapshot() {
                SINK.with(|s| {
                    if let Some(sink) = s.borrow().as_ref() {
                        sink(snapshot);
                    }
                });
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            unsafe {
                let _ = RemoveClipboardFormatListener(hwnd);
                let _ = DestroyWindow(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Opens the clipboard (retrying while another app holds it) and reads formats,
/// marker data and Unicode text.
fn read_snapshot() -> Option<ClipboardSnapshot> {
    let mut opened = false;
    for _ in 0..10 {
        // SAFETY: no owner window needed for reading.
        if unsafe { OpenClipboard(None) }.is_ok() {
            opened = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if !opened {
        log::warn!("clipboard busy, skipped one change");
        return None;
    }

    let source = source_app();
    let mut formats = Vec::new();
    let mut text = None;
    // SAFETY: the clipboard is open for the duration of this block.
    unsafe {
        let mut format = EnumClipboardFormats(0);
        while format != 0 {
            let name = format_name(format);
            let data = if privacy::windows::DATA_FORMATS
                .iter()
                .any(|n| n.eq_ignore_ascii_case(&name))
            {
                read_bytes(format, 16)
            } else {
                None
            };
            formats.push(ClipboardFormat { name, data });
            format = EnumClipboardFormats(format);
        }
        if formats.iter().any(|f| f.name == "CF_UNICODETEXT") {
            text = read_unicode_text();
        }
        let _ = CloseClipboard();
    }
    Some(ClipboardSnapshot {
        text,
        formats,
        source,
    })
}

/// Registered formats have names; predefined ones get a stable `CF_<n>` label.
unsafe fn format_name(format: u32) -> String {
    if format == u32::from(CF_UNICODETEXT.0) {
        return "CF_UNICODETEXT".into();
    }
    let mut buf = [0u16; 256];
    let len = unsafe { GetClipboardFormatNameW(format, &mut buf) };
    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        format!("CF_{format}")
    }
}

unsafe fn read_bytes(format: u32, max: usize) -> Option<Vec<u8>> {
    unsafe {
        let handle = GetClipboardData(format).ok()?;
        let global = HGLOBAL(handle.0);
        let size = GlobalSize(global).min(max);
        let ptr = GlobalLock(global) as *const u8;
        if ptr.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
        let _ = GlobalUnlock(global);
        Some(bytes)
    }
}

unsafe fn read_unicode_text() -> Option<String> {
    unsafe {
        let handle = GetClipboardData(u32::from(CF_UNICODETEXT.0)).ok()?;
        let global = HGLOBAL(handle.0);
        let units = GlobalSize(global) / 2;
        let ptr = GlobalLock(global) as *const u16;
        if ptr.is_null() {
            return None;
        }
        let slice = std::slice::from_raw_parts(ptr, units);
        let end = slice.iter().position(|&c| c == 0).unwrap_or(units);
        let text = String::from_utf16_lossy(&slice[..end]);
        let _ = GlobalUnlock(global);
        Some(text)
    }
}

/// Process of the clipboard owner window, else of the foreground window.
fn source_app() -> Option<SourceApp> {
    // SAFETY: read-only Win32 queries; the process handle is closed below.
    unsafe {
        let hwnd = GetClipboardOwner()
            .ok()
            .filter(|h| !h.is_invalid())
            .unwrap_or_else(|| GetForegroundWindow());
        if hwnd.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(process);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let exe = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_owned();
        let name = exe.strip_suffix(".exe").unwrap_or(&exe).to_owned();
        Some(SourceApp {
            id: exe,
            name: Some(name),
        })
    }
}
