//! Context menu (GTK `PopoverMenu`) model and the rule for when it opens.

use gtk4::gio;

/// `(label, action name)`; actions live in the `skin` group of the window.
pub const ENTRIES: [(&str, &str); 6] = [
    ("Refresh", "refresh"),
    ("Unload", "unload"),
    ("Draggable", "draggable"),
    ("Click-through", "clickthrough"),
    ("Keep on screen", "keeponscreen"),
    ("Open skin folder", "openfolder"),
];

pub fn build_model() -> gio::Menu {
    let menu = gio::Menu::new();
    let top = gio::Menu::new();
    top.append(Some(ENTRIES[0].0), Some(&format!("skin.{}", ENTRIES[0].1)));
    top.append(Some(ENTRIES[1].0), Some(&format!("skin.{}", ENTRIES[1].1)));
    menu.append_section(None, &top);
    let toggles = gio::Menu::new();
    for (label, action) in &ENTRIES[2..5] {
        toggles.append(Some(label), Some(&format!("skin.{action}")));
    }
    menu.append_section(None, &toggles);
    let last = gio::Menu::new();
    last.append(Some(ENTRIES[5].0), Some(&format!("skin.{}", ENTRIES[5].1)));
    menu.append_section(None, &last);
    menu
}

/// As in Rainmeter: a skin that handles right-button actions at the point
/// suppresses the menu; Ctrl+right-click always opens it.
pub fn should_show(skin_handled_right_button: bool, ctrl: bool) -> bool {
    ctrl || !skin_handled_right_button
}

#[cfg(test)]
mod tests {
    use super::*;
    use gtk4::gio::prelude::MenuModelExt;

    #[test]
    fn menu_rule() {
        assert!(should_show(false, false));
        assert!(!should_show(true, false));
        assert!(should_show(true, true));
        assert!(should_show(false, true));
    }

    #[test]
    fn model_has_all_entries() {
        let m = build_model();
        let mut n = 0;
        for i in 0..m.n_items() {
            if let Some(sec) = m.item_link(i, "section") {
                let sec: gio::MenuModel = sec;
                n += sec.n_items();
            }
        }
        assert_eq!(n as usize, ENTRIES.len());
    }
}
