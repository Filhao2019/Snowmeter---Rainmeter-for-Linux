//! One GTK window per active skin: painting, input region, mouse actions,
//! dragging, context menu, tooltips, update timers.

use crate::app::Inner;
use crate::menu;
use crate::position;
use crate::region::{self, RectI};
use crate::x11;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use snowmeter_core::settings::ConfigState;
use snowmeter_core::skin::{AppRequest, MouseButton, MouseEvent};
use snowmeter_core::Skin;
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

const DRAG_THRESHOLD: i32 = 3;

struct Drag {
    start_root: (i32, i32),
    start_win: (i32, i32),
    moved: bool,
    /// Ctrl+drag: the press ran no action, so the release must not either.
    swallow_up: bool,
}

struct WinState {
    cfg: ConfigState,
    anchor: (String, String),
    /// Top-left in root coordinates.
    pos: (i32, i32),
    last_size: (i32, i32),
    rects: Vec<RectI>,
    /// Hash of the alpha plane the current `rects` were built from.
    mask_key: Option<u64>,
    interval: i64,
    drag: Option<Drag>,
    pointer: (f64, f64),
    tooltip: Option<String>,
    shown: bool,
}

pub struct SkinWindow {
    pub config: String,
    pub file: RefCell<String>,
    pub win: gtk4::ApplicationWindow,
    area: gtk4::DrawingArea,
    popover: gtk4::PopoverMenu,
    actions: gio::SimpleActionGroup,
    pub skin: Rc<RefCell<Skin>>,
    st: RefCell<WinState>,
    host: Weak<Inner>,
    alive: Cell<bool>,
    timer_gen: Cell<u32>,
    delay_gen: Cell<u32>,
}

fn map_button(b: u32) -> Option<MouseButton> {
    match b {
        1 => Some(MouseButton::Left),
        2 => Some(MouseButton::Middle),
        3 => Some(MouseButton::Right),
        8 => Some(MouseButton::X1),
        9 => Some(MouseButton::X2),
        _ => None,
    }
}

pub fn install_css() {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string("window.snow, window.snow > * { background: transparent; box-shadow: none; }");
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(&display, &provider, gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }
}

impl SkinWindow {
    pub fn create(host: &Rc<Inner>, config: &str, file: &str, skin: Skin, cfg: ConfigState, anchor: (String, String), start_hidden: bool) -> Rc<SkinWindow> {
        let (w, h) = skin.size();
        let win = gtk4::ApplicationWindow::new(&host.gapp);
        win.set_title(Some(&format!("Snowmeter - {config}")));
        win.set_decorated(false);
        win.set_resizable(false);
        win.add_css_class("snow");
        let area = gtk4::DrawingArea::new();
        area.set_content_width(w.max(1));
        area.set_content_height(h.max(1));
        win.set_child(Some(&area));

        let skin = Rc::new(RefCell::new(skin));
        {
            let skin = skin.clone();
            area.set_draw_func(move |_, cr, _, _| {
                if let Ok(s) = skin.try_borrow() {
                    s.render(cr);
                }
            });
        }
        let popover = gtk4::PopoverMenu::from_model(Some(&menu::build_model()));
        popover.set_parent(&area);
        popover.set_has_arrow(false);
        let group = gio::SimpleActionGroup::new();
        area.insert_action_group("skin", Some(&group));

        let sw = Rc::new(SkinWindow {
            config: config.to_string(),
            file: RefCell::new(file.to_string()),
            win,
            area,
            popover,
            actions: group,
            skin,
            st: RefCell::new(WinState {
                cfg,
                anchor,
                pos: (0, 0),
                last_size: (w, h),
                rects: Vec::new(),
                mask_key: None,
                interval: 0,
                drag: None,
                pointer: (0.0, 0.0),
                tooltip: None,
                shown: !start_hidden,
            }),
            host: Rc::downgrade(host),
            alive: Cell::new(true),
            timer_gen: Cell::new(0),
            delay_gen: Cell::new(0),
        });
        sw.compute_position();
        sw.connect_window(host);
        sw.connect_input();
        sw.connect_menu();
        sw.apply_opacity();
        sw.update_region(true);
        sw.arm_timer();
        sw.after_activity();
        if !start_hidden {
            sw.win.present();
        }
        sw
    }

    fn host(&self) -> Option<Rc<Inner>> {
        self.host.upgrade()
    }

    fn connect_window(self: &Rc<Self>, host: &Rc<Inner>) {
        let kind = host.window_type();
        let aot = self.st.borrow().cfg.always_on_top;
        self.win.connect_realize(move |w| {
            if x11::is_x11() {
                x11::apply_hints(w, kind, aot);
            }
        });
        let weak = Rc::downgrade(self);
        self.win.connect_map(move |w| {
            if let Some(this) = weak.upgrade() {
                let (x, y) = this.st.borrow().pos;
                if x11::is_x11() {
                    x11::move_window(w, x, y);
                }
                this.apply_region();
            }
        });
    }

    // ------------------------------------------------------------ geometry

    fn compute_position(&self) {
        let Some(host) = self.host() else { return };
        let monitors = host.monitors();
        let (w, h) = self.skin.borrow().size();
        let mut st = self.st.borrow_mut();
        let size = (w as f64, h as f64);
        let mut p = position::resolve_position(&st.cfg.window_x, &st.cfg.window_y, &st.anchor.0, &st.anchor.1, size, &monitors);
        if st.cfg.keep_on_screen {
            p = position::keep_on_screen(p, size, &monitors);
        }
        st.pos = (p.0.round() as i32, p.1.round() as i32);
    }

    pub fn position(&self) -> (i32, i32) {
        self.st.borrow().pos
    }

    fn place(&self) {
        let (x, y) = self.st.borrow().pos;
        if x11::is_x11() && self.win.is_mapped() {
            x11::move_window(&self.win, x, y);
        }
    }

    /// Re-read position text from the state (after `!Move`, resize, monitor change).
    pub fn reposition(&self) {
        self.compute_position();
        self.place();
    }

    pub fn move_to(&self, x: f64, y: f64) {
        {
            let mut st = self.st.borrow_mut();
            st.cfg.window_x = format!("{}", x.round() as i64);
            st.cfg.window_y = format!("{}", y.round() as i64);
        }
        self.reposition();
    }

    /// `(WindowX, WindowY)` text to persist.
    pub fn position_text(&self) -> (String, String) {
        let st = self.st.borrow();
        (st.cfg.window_x.clone(), st.cfg.window_y.clone())
    }

    fn store_position_after_move(&self) {
        let Some(host) = self.host() else { return };
        let monitors = host.monitors();
        let (w, h) = self.skin.borrow().size();
        let save;
        let (sx, sy);
        {
            let mut st = self.st.borrow_mut();
            let size = (w as f64, h as f64);
            let mut pos = (st.pos.0 as f64, st.pos.1 as f64);
            if st.cfg.keep_on_screen {
                pos = position::keep_on_screen(pos, size, &monitors);
                st.pos = (pos.0.round() as i32, pos.1.round() as i32);
            }
            let (a, b) = position::format_position(
                (&st.cfg.window_x, &st.cfg.window_y),
                (&st.anchor.0, &st.anchor.1),
                (st.pos.0 as f64, st.pos.1 as f64),
                size,
                &monitors,
            );
            st.cfg.window_x = a.clone();
            st.cfg.window_y = b.clone();
            sx = a;
            sy = b;
            save = st.cfg.save_position;
        }
        self.place();
        if save {
            host.persist_position(&self.config, &sx, &sy);
        }
    }

    // ------------------------------------------------------------ state setters

    pub fn state(&self) -> ConfigState {
        self.st.borrow().cfg.clone()
    }

    pub fn set_draggable(&self, v: bool) {
        self.st.borrow_mut().cfg.draggable = v;
    }

    pub fn set_click_through(&self, v: bool) {
        self.st.borrow_mut().cfg.click_through = v;
        self.update_region(true);
    }

    pub fn set_keep_on_screen(&self, v: bool) {
        self.st.borrow_mut().cfg.keep_on_screen = v;
        if v {
            self.reposition();
        }
    }

    pub fn set_alpha(&self, a: u8) {
        self.st.borrow_mut().cfg.alpha_value = a;
        self.apply_opacity();
    }

    fn apply_opacity(&self) {
        self.win.set_opacity(self.st.borrow().cfg.alpha_value as f64 / 255.0);
    }

    pub fn set_zpos(&self, v: i32) {
        self.st.borrow_mut().cfg.always_on_top = v;
        if x11::is_x11() && self.win.is_mapped() {
            x11::set_wm_state(&self.win, "_NET_WM_STATE_ABOVE", v > 0);
            x11::set_wm_state(&self.win, "_NET_WM_STATE_BELOW", v < 0);
        }
    }

    pub fn set_visible(&self, v: bool) {
        self.st.borrow_mut().shown = v;
        self.win.set_visible(v);
    }

    pub fn is_shown(&self) -> bool {
        self.st.borrow().shown
    }

    /// Close the window; timers stop on their next wake-up.
    pub fn destroy(&self) {
        self.alive.set(false);
        self.popover.unparent();
        self.win.destroy();
    }

    // ------------------------------------------------------------ skin plumbing

    pub fn execute(self: &Rc<Self>, actions: &str) {
        self.skin.borrow_mut().execute(actions);
        self.after_activity();
    }

    pub fn tick(self: &Rc<Self>) {
        self.skin.borrow_mut().tick();
        self.after_activity();
    }

    /// Replace the running skin with a freshly loaded one.
    pub fn replace_skin(self: &Rc<Self>, skin: Skin) {
        *self.skin.borrow_mut() = skin;
        {
            let mut st = self.st.borrow_mut();
            st.rects.clear();
            st.mask_key = None;
        }
        self.area.queue_draw();
        self.update_region(true);
        self.arm_timer();
        self.after_activity();
    }

    /// Drain requests, redraw, resize, schedule `!Delay` and timer changes.
    pub fn after_activity(self: &Rc<Self>) {
        if !self.alive.get() {
            return;
        }
        let (reqs, dirty, size, interval, delay) = {
            let mut s = self.skin.borrow_mut();
            (s.take_requests(), s.take_dirty(), s.size(), s.update_interval_ms(), s.next_delay_ms())
        };
        let resized = {
            let mut st = self.st.borrow_mut();
            let r = st.last_size != size;
            st.last_size = size;
            r
        };
        if resized {
            self.area.set_content_width(size.0.max(1));
            self.area.set_content_height(size.1.max(1));
            self.reposition();
        }
        if dirty || resized {
            self.area.queue_draw();
            self.update_region(false);
        }
        if let Some(ms) = delay {
            self.schedule_delay(ms);
        }
        if interval != self.st.borrow().interval {
            self.arm_timer();
        }
        if !reqs.is_empty() {
            let reqs: Vec<AppRequest> = reqs;
            if let Some(h) = self.host() {
                h.submit(&self.config, reqs);
            }
        }
    }

    fn schedule_delay(self: &Rc<Self>, ms: u64) {
        let gen = self.delay_gen.get().wrapping_add(1);
        self.delay_gen.set(gen);
        let weak = Rc::downgrade(self);
        glib::timeout_add_local_once(Duration::from_millis(ms + 1), move || {
            if let Some(this) = weak.upgrade() {
                if this.alive.get() && this.delay_gen.get() == gen {
                    this.skin.borrow_mut().run_delayed();
                    this.after_activity();
                }
            }
        });
    }

    fn arm_timer(self: &Rc<Self>) {
        let ms = self.skin.borrow().update_interval_ms();
        self.st.borrow_mut().interval = ms;
        let gen = self.timer_gen.get().wrapping_add(1);
        self.timer_gen.set(gen);
        if ms <= 0 {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(ms as u64), move || {
            let Some(this) = weak.upgrade() else { return glib::ControlFlow::Break };
            if !this.alive.get() || this.timer_gen.get() != gen {
                return glib::ControlFlow::Break;
            }
            this.tick();
            glib::ControlFlow::Continue
        });
    }

    // ------------------------------------------------------------ input region

    fn update_region(&self, force: bool) {
        let (rects, key) = if self.st.borrow().cfg.click_through {
            (Vec::new(), None)
        } else {
            let (w, h, alpha) = self.skin.borrow().hit_mask();
            let key = region::mask_key(w, h, &alpha);
            // Same pixels as last time: the rectangles cannot have changed.
            if !force && self.st.borrow().mask_key == Some(key) {
                return;
            }
            (region::mask_rects(w, h, &alpha), Some(key))
        };
        {
            let mut st = self.st.borrow_mut();
            st.mask_key = key;
            if !force && st.rects == rects {
                return;
            }
            st.rects = rects;
        }
        self.apply_region();
    }

    fn apply_region(&self) {
        if let Some(surface) = self.win.surface() {
            let r = region::to_region(&self.st.borrow().rects);
            surface.set_input_region(Some(&r));
        }
    }

    // ------------------------------------------------------------ input

    fn local(&self, x: f64, y: f64) -> (f64, f64) {
        (x, y)
    }

    fn send_mouse(self: &Rc<Self>, ev: MouseEvent) -> bool {
        let pos = self.st.borrow().pos;
        let handled = {
            let mut skin = self.skin.borrow_mut();
            skin.set_screen_position(pos.0 as f64, pos.1 as f64);
            skin.mouse(ev)
        };
        self.after_activity();
        handled
    }

    fn connect_input(self: &Rc<Self>) {
        let click = gtk4::GestureClick::new();
        click.set_button(0);
        {
            let weak = Rc::downgrade(self);
            click.connect_pressed(move |g, n, x, y| {
                if let Some(this) = weak.upgrade() {
                    let ctrl = g.current_event_state().contains(gdk::ModifierType::CONTROL_MASK);
                    this.on_press(g.current_button(), n, x, y, ctrl);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            click.connect_released(move |g, _n, x, y| {
                if let Some(this) = weak.upgrade() {
                    let ctrl = g.current_event_state().contains(gdk::ModifierType::CONTROL_MASK);
                    this.on_release(g.current_button(), x, y, ctrl);
                }
            });
        }
        self.area.add_controller(click);

        let motion = gtk4::EventControllerMotion::new();
        {
            let weak = Rc::downgrade(self);
            motion.connect_motion(move |_, x, y| {
                if let Some(this) = weak.upgrade() {
                    this.on_motion(x, y);
                }
            });
        }
        {
            let weak = Rc::downgrade(self);
            motion.connect_leave(move |_| {
                if let Some(this) = weak.upgrade() {
                    if this.st.borrow().drag.is_none() {
                        this.send_mouse(MouseEvent::Leave);
                    }
                }
            });
        }
        self.area.add_controller(motion);

        let scroll = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::BOTH_AXES | gtk4::EventControllerScrollFlags::DISCRETE);
        {
            let weak = Rc::downgrade(self);
            scroll.connect_scroll(move |_, dx, dy| {
                let Some(this) = weak.upgrade() else { return glib::Propagation::Proceed };
                let (x, y) = this.st.borrow().pointer;
                let sdx = if dx > 0.0 { 1 } else if dx < 0.0 { -1 } else { 0 };
                // GTK: dy > 0 scrolls down; the skin API: dy > 0 is up.
                let sdy = if dy > 0.0 { -1 } else if dy < 0.0 { 1 } else { 0 };
                if this.send_mouse(MouseEvent::Scroll { dx: sdx, dy: sdy, x, y }) {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            });
        }
        self.area.add_controller(scroll);
    }

    fn on_press(self: &Rc<Self>, button: u32, n_press: i32, x: f64, y: f64, ctrl: bool) {
        let Some(btn) = map_button(button) else { return };
        let (x, y) = self.local(x, y);
        self.st.borrow_mut().pointer = (x, y);
        if btn == MouseButton::Left {
            let draggable = self.st.borrow().cfg.draggable;
            if ctrl {
                if draggable {
                    self.begin_drag(true);
                }
                return;
            }
            let ev = if n_press >= 2 { MouseEvent::DoubleClick { button: btn, x, y } } else { MouseEvent::Down { button: btn, x, y } };
            let handled = self.send_mouse(ev);
            if n_press < 2 && !handled && draggable {
                self.begin_drag(false);
            }
        } else {
            let ev = if n_press >= 2 { MouseEvent::DoubleClick { button: btn, x, y } } else { MouseEvent::Down { button: btn, x, y } };
            self.send_mouse(ev);
        }
    }

    fn on_release(self: &Rc<Self>, button: u32, x: f64, y: f64, ctrl: bool) {
        let Some(btn) = map_button(button) else { return };
        let (x, y) = self.local(x, y);
        match btn {
            MouseButton::Left => {
                let drag = self.st.borrow_mut().drag.take();
                match drag {
                    Some(d) if d.moved => self.store_position_after_move(),
                    Some(d) if d.swallow_up => {}
                    _ => {
                        if !ctrl {
                            self.send_mouse(MouseEvent::Up { button: btn, x, y });
                        }
                    }
                }
            }
            MouseButton::Right => {
                let handled = self.send_mouse(MouseEvent::Up { button: btn, x, y });
                if menu::should_show(handled, ctrl) {
                    self.show_menu(x, y);
                }
            }
            _ => {
                self.send_mouse(MouseEvent::Up { button: btn, x, y });
            }
        }
    }

    fn begin_drag(&self, swallow_up: bool) {
        if !x11::is_x11() {
            return;
        }
        if let Some(root) = x11::pointer_root(&self.win) {
            let start_win = self.st.borrow().pos;
            self.st.borrow_mut().drag = Some(Drag { start_root: root, start_win, moved: false, swallow_up });
        }
    }

    fn on_motion(self: &Rc<Self>, x: f64, y: f64) {
        self.st.borrow_mut().pointer = (x, y);
        let dragging = self.st.borrow().drag.is_some();
        if dragging {
            if let Some((rx, ry)) = x11::pointer_root(&self.win) {
                let mut st = self.st.borrow_mut();
                if let Some(d) = st.drag.as_mut() {
                    let (dx, dy) = (rx - d.start_root.0, ry - d.start_root.1);
                    if !d.moved && dx.abs().max(dy.abs()) >= DRAG_THRESHOLD {
                        d.moved = true;
                    }
                    if d.moved {
                        let np = (d.start_win.0 + dx, d.start_win.1 + dy);
                        st.pos = np;
                        drop(st);
                        x11::move_window(&self.win, np.0, np.1);
                    }
                }
            }
            return;
        }
        self.send_mouse(MouseEvent::Move { x, y });
        self.update_tooltip(x, y);
    }

    fn update_tooltip(&self, x: f64, y: f64) {
        let tip = {
            let s = self.skin.borrow();
            s.meter_names().iter().rev().find_map(|n| {
                let b = s.meter_base(n)?;
                if !b.hidden && !b.tooltip_text.is_empty() && b.hit_test(x, y) {
                    Some(if b.tooltip_title.is_empty() { b.tooltip_text.clone() } else { format!("{}\n{}", b.tooltip_title, b.tooltip_text) })
                } else {
                    None
                }
            })
        };
        let mut st = self.st.borrow_mut();
        if st.tooltip != tip {
            self.area.set_tooltip_text(tip.as_deref());
            st.tooltip = tip;
        }
    }

    // ------------------------------------------------------------ context menu

    fn connect_menu(self: &Rc<Self>) {
        let add_plain = |name: &str, req: Box<dyn Fn(&SkinWindow) -> Vec<AppRequest>>| {
            let a = gio::SimpleAction::new(name, None);
            let weak = Rc::downgrade(self);
            a.connect_activate(move |_, _| {
                if let Some(this) = weak.upgrade() {
                    let reqs = req(&this);
                    if let Some(h) = this.host() {
                        h.submit(&this.config, reqs);
                    }
                }
            });
            self.actions.add_action(&a);
        };
        add_plain("refresh", Box::new(|w| vec![AppRequest::Refresh { config: Some(w.config.clone()) }]));
        add_plain("unload", Box::new(|w| vec![AppRequest::Deactivate { config: w.config.clone() }]));
        add_plain("openfolder", Box::new(|w| vec![AppRequest::OpenOrRun(w.folder().to_string_lossy().into_owned())]));

        let toggles: [(&str, fn(&ConfigState) -> bool); 3] =
            [("draggable", |c| c.draggable), ("clickthrough", |c| c.click_through), ("keeponscreen", |c| c.keep_on_screen)];
        for (name, get) in toggles {
            let a = gio::SimpleAction::new_stateful(name, None, &get(&self.st.borrow().cfg).to_variant());
            let weak = Rc::downgrade(self);
            a.connect_change_state(move |act, v| {
                let (Some(this), Some(v)) = (weak.upgrade(), v) else { return };
                let on = v.get::<bool>().unwrap_or(false);
                act.set_state(v);
                if let Some(h) = this.host() {
                    h.menu_toggle(&this.config, name, on);
                }
            });
            self.actions.add_action(&a);
        }
    }

    fn folder(&self) -> std::path::PathBuf {
        self.skin.borrow().ini_path().parent().map(|p| p.to_path_buf()).unwrap_or_default()
    }

    fn show_menu(&self, x: f64, y: f64) {
        let cfg = self.st.borrow().cfg.clone();
        for (name, v) in [("draggable", cfg.draggable), ("clickthrough", cfg.click_through), ("keeponscreen", cfg.keep_on_screen)] {
            if let Some(a) = self.actions.lookup_action(name) {
                if let Some(a) = a.downcast_ref::<gio::SimpleAction>() {
                    a.set_state(&v.to_variant());
                }
            }
        }
        self.popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        self.popover.popup();
    }
}
