//! Native InputText: a borderless entry window placed over the skin.
//! Enter runs the commands with `$UserInput$` replaced; Esc or (with
//! `FocusDismiss`) losing focus dismisses it.

use crate::x11;
use gtk4::prelude::*;
use snowmeter_core::color::Color;
use snowmeter_core::measures::inputtext::InputTextSpec;
use std::cell::RefCell;

const TOKEN: &str = "$UserInput$";

/// Replace `$UserInput$` (any case) in every command.
pub fn expand_commands(commands: &[String], text: &str) -> Vec<String> {
    commands.iter().map(|c| replace_ci(c, TOKEN, text)).collect()
}

fn replace_ci(s: &str, pat: &str, with: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let pat_l = pat.to_ascii_lowercase();
    let mut out = String::new();
    let mut i = 0;
    while let Some(pos) = lower[i..].find(&pat_l) {
        out.push_str(&s[i..i + pos]);
        out.push_str(with);
        i += pos + pat.len();
    }
    out.push_str(&s[i..]);
    out
}

fn css_color(c: Color) -> String {
    format!("rgba({},{},{},{:.3})", c.r, c.g, c.b, c.a as f64 / 255.0)
}

pub fn css_for(spec: &InputTextSpec) -> String {
    let family = spec.font_face.chars().filter(|c| !matches!(c, '\'' | '"' | '\\' | '{' | '}' | ';')).collect::<String>();
    format!(
        "window.snow-input {{ background: {bg}; }} \
         window.snow-input entry {{ background: {bg}; color: {fg}; font-family: '{family}'; font-size: {size}pt; \
         min-height: 0; padding: 0 2px; border: none; box-shadow: none; outline: none; border-radius: 0; }} \
         window.snow-input entry text {{ color: {fg}; }}",
        bg = css_color(spec.solid_color),
        fg = css_color(spec.font_color),
        size = if spec.font_size.is_finite() { spec.font_size.clamp(1.0, 500.0) } else { 8.0 }
    )
}

thread_local! {
    static CURRENT: RefCell<Option<gtk4::Window>> = const { RefCell::new(None) };
}

/// Show the entry. `origin` is the skin window's top-left in root coordinates.
pub fn show(spec: &InputTextSpec, origin: (i32, i32), on_submit: impl Fn(String) + 'static) {
    if let Some(old) = CURRENT.with(|c| c.borrow_mut().take()) {
        old.close();
    }
    let win = gtk4::Window::new();
    win.set_decorated(false);
    win.set_resizable(false);
    win.add_css_class("snow-input");
    win.set_default_size(spec.w.max(20), spec.h.max(12));
    let entry = gtk4::Entry::new();
    entry.set_has_frame(false);
    entry.set_hexpand(true);
    entry.set_text(&spec.default_value);
    win.set_child(Some(&entry));

    let provider = gtk4::CssProvider::new();
    provider.load_from_string(&css_for(spec));
    if let Some(display) = gtk4::gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_USER);
    }

    let pos = (origin.0 + spec.x, origin.1 + spec.y);
    win.connect_map(move |w| x11::move_window(w, pos.0, pos.1));

    let submitted = std::rc::Rc::new(std::cell::Cell::new(false));
    {
        let win = win.clone();
        let submitted = submitted.clone();
        entry.connect_activate(move |e| {
            if submitted.replace(true) {
                return;
            }
            on_submit(e.text().to_string());
            win.close();
        });
    }
    let keys = gtk4::EventControllerKey::new();
    {
        let win = win.clone();
        keys.connect_key_pressed(move |_, key, _, _| {
            if key == gtk4::gdk::Key::Escape {
                win.close();
                gtk4::glib::Propagation::Stop
            } else {
                gtk4::glib::Propagation::Proceed
            }
        });
    }
    entry.add_controller(keys);
    if spec.focus_dismiss {
        let armed = std::rc::Rc::new(std::cell::Cell::new(false));
        let a = armed.clone();
        win.connect_is_active_notify(move |w| {
            if w.is_active() {
                a.set(true);
            } else if a.get() {
                w.close();
            }
        });
    }
    win.connect_close_request(move |_| {
        if let Some(display) = gtk4::gdk::Display::default() {
            gtk4::style_context_remove_provider_for_display(&display, &provider);
        }
        CURRENT.with(|c| *c.borrow_mut() = None);
        gtk4::glib::Propagation::Proceed
    });
    CURRENT.with(|c| *c.borrow_mut() = Some(win.clone()));
    win.present();
    entry.grab_focus();
    entry.select_region(0, -1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_user_input() {
        let c = vec!["[\"http://g/?q=$UserInput$\"]".to_string(), "[!SetVariable A \"$userinput$$UserInput$\"]".to_string()];
        let out = expand_commands(&c, "a b");
        assert_eq!(out[0], "[\"http://g/?q=a b\"]");
        assert_eq!(out[1], "[!SetVariable A \"a ba b\"]");
        assert_eq!(expand_commands(&["x".into()], "y"), vec!["x"]);
    }

    #[test]
    fn css_has_colors() {
        let spec = InputTextSpec {
            measure: "m".into(),
            x: 0,
            y: 0,
            w: 10,
            h: 10,
            solid_color: Color { r: 20, g: 20, b: 20, a: 255 },
            font_color: Color { r: 255, g: 255, b: 255, a: 128 },
            font_face: "Trebuchet MS".into(),
            font_size: 8.0,
            default_value: String::new(),
            focus_dismiss: true,
            commands: vec![],
        };
        let css = css_for(&spec);
        assert!(css.contains("rgba(20,20,20,1.000)") && css.contains("Trebuchet MS") && css.contains("8pt"));
    }
}
