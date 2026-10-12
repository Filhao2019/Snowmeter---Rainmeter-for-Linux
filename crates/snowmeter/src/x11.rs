//! Xlib helpers for the X11 session: window type and state hints, moving
//! windows (GTK4 has no move API) and reading the pointer in root coordinates.

use gdk4_x11::{X11Display, X11Surface};
use gtk4::prelude::*;
use snowmeter_core::settings::WindowType;
use std::ffi::CString;
use std::os::raw::{c_int, c_long, c_uchar, c_ulong};
use x11::xlib;

/// True when GDK runs on X11.
pub fn is_x11() -> bool {
    gtk4::gdk::Display::default().map(|d| d.is::<X11Display>()).unwrap_or(false)
}

struct Handle {
    display: *mut xlib::Display,
    xid: xlib::Window,
}

fn handle(win: &impl IsA<gtk4::Native>) -> Option<Handle> {
    let surf = win.surface()?;
    let x11surf = surf.downcast_ref::<X11Surface>()?;
    let xid = x11surf.xid();
    let disp = surf.display().downcast::<X11Display>().ok()?;
    let display = unsafe { disp.xdisplay() };
    if display.is_null() {
        return None;
    }
    Some(Handle { display: display as *mut xlib::Display, xid })
}

unsafe fn atom(d: *mut xlib::Display, name: &str) -> xlib::Atom {
    let c = CString::new(name).unwrap();
    xlib::XInternAtom(d, c.as_ptr(), xlib::False)
}

unsafe fn set_atom_list(h: &Handle, property: &str, values: &[&str]) {
    let prop = atom(h.display, property);
    let atoms: Vec<xlib::Atom> = values.iter().map(|v| atom(h.display, v)).collect();
    xlib::XChangeProperty(
        h.display,
        h.xid,
        prop,
        xlib::XA_ATOM,
        32,
        xlib::PropModeReplace,
        atoms.as_ptr() as *const c_uchar,
        atoms.len() as c_int,
    );
}

/// `_NET_WM_STATE` list for a window of the given type and `AlwaysOnTop` value.
pub fn wm_states(kind: WindowType, always_on_top: i32) -> Vec<&'static str> {
    let mut v = vec!["_NET_WM_STATE_STICKY", "_NET_WM_STATE_SKIP_TASKBAR", "_NET_WM_STATE_SKIP_PAGER"];
    match always_on_top {
        1 | 2 => v.push("_NET_WM_STATE_ABOVE"),
        -1 => v.push("_NET_WM_STATE_BELOW"),
        _ => {
            if kind != WindowType::Normal {
                v.push("_NET_WM_STATE_BELOW");
            }
        }
    }
    v
}

/// Call from `realize` (before the window is mapped).
pub fn apply_hints(win: &impl IsA<gtk4::Native>, kind: WindowType, always_on_top: i32) {
    let Some(h) = handle(win) else { return };
    unsafe {
        let desktop = kind == WindowType::Desktop && !matches!(always_on_top, 1 | 2 | -1);
        if desktop {
            set_atom_list(&h, "_NET_WM_WINDOW_TYPE", &["_NET_WM_WINDOW_TYPE_DESKTOP"]);
        }
        if kind != WindowType::Normal || always_on_top != 0 {
            set_atom_list(&h, "_NET_WM_STATE", &wm_states(kind, always_on_top));
        }
        xlib::XFlush(h.display);
    }
}

/// Add or remove one `_NET_WM_STATE_*` on a mapped window.
pub fn set_wm_state(win: &impl IsA<gtk4::Native>, state: &str, add: bool) {
    let Some(h) = handle(win) else { return };
    unsafe {
        let mut ev: xlib::XClientMessageEvent = std::mem::zeroed();
        ev.type_ = xlib::ClientMessage;
        ev.window = h.xid;
        ev.message_type = atom(h.display, "_NET_WM_STATE");
        ev.format = 32;
        ev.data.set_long(0, if add { 1 } else { 0 });
        ev.data.set_long(1, atom(h.display, state) as c_long);
        ev.data.set_long(2, 0);
        ev.data.set_long(3, 1);
        let root = xlib::XDefaultRootWindow(h.display);
        let mut e: xlib::XEvent = ev.into();
        xlib::XSendEvent(
            h.display,
            root,
            xlib::False,
            xlib::SubstructureRedirectMask | xlib::SubstructureNotifyMask,
            &mut e,
        );
        xlib::XFlush(h.display);
    }
}

pub fn move_window(win: &impl IsA<gtk4::Native>, x: i32, y: i32) {
    let Some(h) = handle(win) else { return };
    unsafe {
        xlib::XMoveWindow(h.display, h.xid, x, y);
        xlib::XFlush(h.display);
    }
}

/// Pointer position in root (screen) coordinates.
pub fn pointer_root(win: &impl IsA<gtk4::Native>) -> Option<(i32, i32)> {
    let h = handle(win)?;
    unsafe {
        let root = xlib::XDefaultRootWindow(h.display);
        let (mut r, mut c): (c_ulong, c_ulong) = (0, 0);
        let (mut rx, mut ry, mut wx, mut wy): (c_int, c_int, c_int, c_int) = (0, 0, 0, 0);
        let mut mask: std::os::raw::c_uint = 0;
        let ok = xlib::XQueryPointer(h.display, root, &mut r, &mut c, &mut rx, &mut ry, &mut wx, &mut wy, &mut mask);
        if ok == 0 {
            None
        } else {
            Some((rx, ry))
        }
    }
}

/// Text of a window property as the WM stores it: UTF-8 (`_NET_WM_NAME`) or
/// Latin-1 (`WM_NAME`), cut at the first NUL.
pub fn decode_title(bytes: &[u8], utf8: bool) -> String {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    let b = &bytes[..end];
    if utf8 {
        String::from_utf8_lossy(b).into_owned()
    } else {
        b.iter().map(|c| *c as char).collect()
    }
}

unsafe fn read_property(d: *mut xlib::Display, win: xlib::Window, prop: xlib::Atom, ty: xlib::Atom, max_words: c_long) -> Option<(Vec<u8>, c_ulong, c_int)> {
    let (mut actual, mut fmt, mut n, mut after): (xlib::Atom, c_int, c_ulong, c_ulong) = (0, 0, 0, 0);
    let mut data: *mut c_uchar = std::ptr::null_mut();
    let rc = xlib::XGetWindowProperty(d, win, prop, 0, max_words, xlib::False, ty, &mut actual, &mut fmt, &mut n, &mut after, &mut data);
    if rc != 0 || data.is_null() {
        return None;
    }
    let bytes_per = match fmt {
        8 => 1,
        16 => 2,
        _ => std::mem::size_of::<c_long>(),
    };
    let out = if actual == 0 || n == 0 { None } else { Some((std::slice::from_raw_parts(data, n as usize * bytes_per).to_vec(), n, fmt)) };
    xlib::XFree(data as *mut _);
    out
}

/// Title of the focused top-level window (`_NET_ACTIVE_WINDOW`, `_NET_WM_NAME`
/// with a `WM_NAME` fallback); empty when nothing is focused.
pub fn active_window_title() -> String {
    let Some(disp) = gtk4::gdk::Display::default().and_then(|d| d.downcast::<X11Display>().ok()) else { return String::new() };
    let d = unsafe { disp.xdisplay() } as *mut xlib::Display;
    if d.is_null() {
        return String::new();
    }
    unsafe {
        let root = xlib::XDefaultRootWindow(d);
        let Some((w, _, _)) = read_property(d, root, atom(d, "_NET_ACTIVE_WINDOW"), xlib::XA_WINDOW, 1) else { return String::new() };
        let mut id = [0u8; std::mem::size_of::<c_long>()];
        id.copy_from_slice(&w[..std::mem::size_of::<c_long>()]);
        let win = c_long::from_ne_bytes(id) as xlib::Window;
        if win == 0 {
            return String::new();
        }
        if let Some((b, _, _)) = read_property(d, win, atom(d, "_NET_WM_NAME"), atom(d, "UTF8_STRING"), 1024) {
            return decode_title(&b, true);
        }
        match read_property(d, win, xlib::XA_WM_NAME, xlib::AnyPropertyType as xlib::Atom, 1024) {
            Some((b, _, _)) => decode_title(&b, false),
            None => String::new(),
        }
    }
}

/// Publish the focused window title twice a second while a skin asks for it.
pub fn start_active_title_poll() {
    if !is_x11() {
        return;
    }
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(500), || {
        if snowmeter_core::activetitle::wanted_within(10_000) {
            snowmeter_core::activetitle::publish(&active_window_title());
        }
        gtk4::glib::ControlFlow::Continue
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_decoding() {
        assert_eq!(decode_title("Caf\u{e9} \u{2014} x".as_bytes(), true), "Caf\u{e9} \u{2014} x");
        assert_eq!(decode_title(&[0x43, 0x61, 0x66, 0xe9, 0, 0x7a], false), "Caf\u{e9}");
        assert_eq!(decode_title(b"", true), "");
        assert_eq!(decode_title(&[0xff, 0x41], true), "\u{fffd}A");
    }

    #[test]
    fn states_per_type() {
        let d = wm_states(WindowType::Desktop, 0);
        assert!(d.contains(&"_NET_WM_STATE_BELOW") && d.contains(&"_NET_WM_STATE_STICKY"));
        let n = wm_states(WindowType::Normal, 0);
        assert!(!n.contains(&"_NET_WM_STATE_BELOW"));
        assert!(wm_states(WindowType::Desktop, 1).contains(&"_NET_WM_STATE_ABOVE"));
        assert!(wm_states(WindowType::Below, 0).contains(&"_NET_WM_STATE_BELOW"));
    }
}
