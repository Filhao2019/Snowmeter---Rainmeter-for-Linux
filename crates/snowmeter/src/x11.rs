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

#[cfg(test)]
mod tests {
    use super::*;

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
