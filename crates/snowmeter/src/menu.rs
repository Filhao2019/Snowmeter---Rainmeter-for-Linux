//! Context menu (GTK `PopoverMenu`) model and the rule for when it opens.

use gtk4::gio;
use gtk4::prelude::*;

/// `(label, action name)`; actions live in the `skin` group of the window.
pub const ENTRIES: [(&str, &str); 7] = [
    ("Refresh", "refresh"),
    ("Edit skin", "edit"),
    ("Unload", "unload"),
    ("Draggable", "draggable"),
    ("Click-through", "clickthrough"),
    ("Keep on screen", "keeponscreen"),
    ("Open skin folder", "openfolder"),
];

/// Sizes offered in the "Size" submenu, in percent of the skin's own size.
pub const SCALES: [u32; 8] = [50, 75, 100, 125, 150, 200, 250, 300];

/// Step for Ctrl+mouse wheel resizing, in percent.
pub const SCALE_STEP: u32 = 10;

/// What the menu shows besides the fixed entries.
pub struct Spec<'a> {
    /// `(title, bang)` from `ContextTitle`/`ContextAction`; an empty title is a separator.
    pub custom: &'a [(String, String)],
    /// `!SkinCustomMenu`: only the custom entries.
    pub custom_only: bool,
    /// `.ini` files of the config; more than one makes a "Variants" submenu.
    pub variants: &'a [String],
}

fn item(label: &str, action: &str) -> gio::MenuItem {
    gio::MenuItem::new(Some(label), Some(&format!("skin.{action}")))
}

/// Custom entries, split into sections at separators.
fn custom_sections(custom: &[(String, String)]) -> gio::Menu {
    let menu = gio::Menu::new();
    let mut sec = gio::Menu::new();
    for (i, (title, _)) in custom.iter().enumerate() {
        if title.is_empty() {
            if sec.n_items() > 0 {
                menu.append_section(None, &sec);
                sec = gio::Menu::new();
            }
            continue;
        }
        let it = gio::MenuItem::new(Some(title), None);
        it.set_action_and_target_value(Some("skin.custom"), Some(&(i as i32).to_variant()));
        sec.append_item(&it);
    }
    if sec.n_items() > 0 {
        menu.append_section(None, &sec);
    }
    menu
}

pub fn build_model(spec: &Spec) -> gio::Menu {
    let has_custom = spec.custom.iter().any(|(t, _)| !t.is_empty());
    if spec.custom_only {
        return custom_sections(spec.custom);
    }
    let menu = gio::Menu::new();
    if has_custom {
        // As in Rainmeter: up to three entries inline, more in a submenu.
        let n = spec.custom.iter().filter(|(t, _)| !t.is_empty()).count();
        if n <= 3 {
            let secs = custom_sections(spec.custom);
            for i in 0..secs.n_items() {
                if let Some(sec) = secs.item_link(i, "section") {
                    menu.append_section(None, &sec);
                }
            }
        } else {
            let sec = gio::Menu::new();
            sec.append_submenu(Some("Skin actions"), &custom_sections(spec.custom));
            menu.append_section(None, &sec);
        }
    }
    let layout = gio::Menu::new();
    if spec.variants.len() > 1 {
        let v = gio::Menu::new();
        for f in spec.variants {
            let it = gio::MenuItem::new(Some(f.trim_end_matches(".ini")), None);
            it.set_action_and_target_value(Some("skin.variant"), Some(&f.to_variant()));
            v.append_item(&it);
        }
        layout.append_submenu(Some("Variants"), &v);
    }
    let sizes = gio::Menu::new();
    for pct in SCALES {
        let it = gio::MenuItem::new(Some(&format!("{pct}%")), None);
        it.set_action_and_target_value(Some("skin.scale"), Some(&pct.to_string().to_variant()));
        sizes.append_item(&it);
    }
    layout.append_submenu(Some("Size"), &sizes);
    menu.append_section(None, &layout);
    let top = gio::Menu::new();
    for (label, action) in &ENTRIES[0..3] {
        top.append_item(&item(label, action));
    }
    menu.append_section(None, &top);
    let toggles = gio::Menu::new();
    for (label, action) in &ENTRIES[3..6] {
        toggles.append_item(&item(label, action));
    }
    menu.append_section(None, &toggles);
    let last = gio::Menu::new();
    last.append_item(&item(ENTRIES[6].0, ENTRIES[6].1));
    menu.append_section(None, &last);
    menu
}

/// Next Ctrl+wheel size: the next multiple of the step above (or below) the
/// current size, clamped to the allowed range.
pub fn step_scale(current: u32, up: bool) -> u32 {
    let (lo, hi) = snowmeter_core::settings::SCALE_RANGE;
    let n = if up { (current / SCALE_STEP + 1) * SCALE_STEP } else { current.saturating_sub(1) / SCALE_STEP * SCALE_STEP };
    n.clamp(lo, hi)
}

/// As in Rainmeter: a skin that handles right-button actions at the point
/// suppresses the menu; Ctrl+right-click always opens it.
pub fn should_show(skin_handled_right_button: bool, ctrl: bool) -> bool {
    ctrl || !skin_handled_right_button
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_rule() {
        assert!(should_show(false, false));
        assert!(!should_show(true, false));
        assert!(should_show(true, true));
        assert!(should_show(false, true));
    }

    fn count(m: &gio::MenuModel) -> usize {
        let mut n = 0;
        for i in 0..m.n_items() {
            match m.item_link(i, "section").or_else(|| m.item_link(i, "submenu")) {
                Some(sub) => n += count(&sub) + usize::from(m.item_link(i, "submenu").is_some()),
                None => n += 1,
            }
        }
        n
    }

    fn s(v: &[(&str, &str)]) -> Vec<(String, String)> {
        v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn plain_menu_has_fixed_entries_and_sizes() {
        let m = build_model(&Spec { custom: &[], custom_only: false, variants: &["A.ini".into()] });
        // fixed entries + "Size" submenu with its sizes
        assert_eq!(count(m.upcast_ref()), ENTRIES.len() + 1 + SCALES.len());
    }

    #[test]
    fn variants_and_inline_custom_entries() {
        let custom = s(&[("One", "[!Log 1]"), ("", ""), ("Two", "[!Log 2]")]);
        let v = vec!["Small.ini".to_string(), "Large.ini".to_string()];
        let m = build_model(&Spec { custom: &custom, custom_only: false, variants: &v });
        assert_eq!(count(m.upcast_ref()), 2 + ENTRIES.len() + 1 + SCALES.len() + 1 + 2);
    }

    #[test]
    fn custom_only_menu_and_submenu_for_many() {
        let custom = s(&[("A", "x"), ("B", "x"), ("", ""), ("C", "x"), ("D", "x")]);
        let m = build_model(&Spec { custom: &custom, custom_only: true, variants: &[] });
        assert_eq!(count(m.upcast_ref()), 4);
        assert_eq!(m.n_items(), 2, "a separator splits sections");
        let full = build_model(&Spec { custom: &custom, custom_only: false, variants: &[] });
        assert_eq!(count(full.upcast_ref()), 1 + 4 + ENTRIES.len() + 1 + SCALES.len());
    }

    #[test]
    fn wheel_steps_snap_and_clamp() {
        assert_eq!(step_scale(100, true), 110);
        assert_eq!(step_scale(125, true), 130);
        assert_eq!(step_scale(125, false), 120);
        assert_eq!(step_scale(100, false), 90);
        assert_eq!(step_scale(30, false), 25);
        assert_eq!(step_scale(400, true), 400);
    }
}
