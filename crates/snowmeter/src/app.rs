//! The GTK application: windows for every active config, IPC and tray
//! messages, `AppRequest` handling, persistence to `snowmeter.ini`.

use crate::commands::{active_from_settings, format_list};
use crate::firstrun;
use crate::inputtext;
use crate::ipc::{self, Request};
use crate::tray::{Tray, TrayCmd};
use crate::window::{self, SkinWindow};
use crate::x11;
use gtk4::prelude::*;
use gtk4::{gdk, gio, glib};
use snowmeter_core::env::{Environment, Rect};
use snowmeter_core::gallery;
use snowmeter_core::registry::SkinRegistry;
use snowmeter_core::rmskin::activate_skin;
use snowmeter_core::settings::{ConfigState, Settings, WindowType};
use snowmeter_core::skin::AppRequest;
use snowmeter_core::Skin;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

enum Msg {
    Ipc(Request, mpsc::Sender<String>),
    Tray(TrayCmd),
    /// A background gallery install finished: `(entry, Ok(report) | Err(message))`.
    GalleryDone(String, Result<String, String>),
}

pub struct Inner {
    pub gapp: gtk4::Application,
    env: RefCell<Environment>,
    settings: RefCell<Settings>,
    registry: RefCell<SkinRegistry>,
    windows: RefCell<Vec<Rc<SkinWindow>>>,
    queue: RefCell<VecDeque<(String, AppRequest)>>,
    draining: Cell<bool>,
    tray: RefCell<Option<Tray>>,
    window_type: Cell<WindowType>,
    sock: std::path::PathBuf,
    tx: mpsc::Sender<Msg>,
    installing: RefCell<Vec<String>>,
}

/// How an `OpenOrRun` string is launched.
#[derive(Debug, PartialEq, Eq)]
pub enum Launch {
    Open(String),
    Run(Vec<String>),
}

/// Split a command line into words, honouring double quotes.
pub fn split_words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_q = false;
    let mut any = false;
    for c in s.chars() {
        match c {
            '"' => {
                in_q = !in_q;
                any = true;
            }
            c if c.is_whitespace() && !in_q => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            c => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn classify_command(s: &str) -> Launch {
    use std::os::unix::fs::PermissionsExt;
    let t = s.trim();
    let unq = t.trim_matches('"');
    if unq.contains("://") || unq.starts_with("mailto:") {
        return Launch::Open(unq.to_string());
    }
    let p = std::path::Path::new(unq);
    if p.is_dir() {
        return Launch::Open(unq.to_string());
    }
    if p.is_file() {
        let exec = p.metadata().map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false);
        return if exec { Launch::Run(vec![unq.to_string()]) } else { Launch::Open(unq.to_string()) };
    }
    Launch::Run(split_words(t))
}

fn spawn_detached(program: &str, args: &[String]) {
    match std::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => log::warn!("cannot run '{program}': {e}"),
    }
}

/// Permanently empty the XDG trash (no confirmation; callers ask first).
fn empty_trash() {
    spawn_detached("gio", &["trash".to_string(), "--empty".to_string()]);
}

pub fn open_or_run(s: &str) {
    match classify_command(s) {
        Launch::Open(t) => spawn_detached("xdg-open", &[t]),
        Launch::Run(words) => {
            if let Some((prog, args)) = words.split_first() {
                spawn_detached(prog, args);
            }
        }
    }
}

fn read_monitors() -> Vec<Rect> {
    let mut v = Vec::new();
    if let Some(display) = gdk::Display::default() {
        let list = display.monitors();
        for i in 0..list.n_items() {
            if let Some(m) = list.item(i).and_then(|o| o.downcast::<gdk::Monitor>().ok()) {
                let g = m.geometry();
                v.push(Rect { x: g.x() as f64, y: g.y() as f64, w: g.width() as f64, h: g.height() as f64 });
            }
        }
    }
    if v.is_empty() {
        v.push(Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 });
    }
    v
}

fn norm(config: &str) -> String {
    config.replace('/', "\\")
}

impl Inner {
    pub fn window_type(&self) -> WindowType {
        self.window_type.get()
    }

    pub fn monitors(&self) -> Vec<Rect> {
        self.env.borrow().monitors.clone()
    }

    fn refresh_monitors(&self) {
        let m = read_monitors();
        let mut env = self.env.borrow_mut();
        env.monitors = m.clone();
        env.work_areas = m;
    }

    fn find(&self, config: &str) -> Option<Rc<SkinWindow>> {
        let c = norm(config);
        self.windows.borrow().iter().find(|w| w.config.eq_ignore_ascii_case(&c)).cloned()
    }

    fn all(&self) -> Vec<Rc<SkinWindow>> {
        self.windows.borrow().clone()
    }

    pub fn persist_position(&self, config: &str, x: &str, y: &str) {
        let mut s = self.settings.borrow_mut();
        s.set_config_key(config, "WindowX", x);
        s.set_config_key(config, "WindowY", y);
        self.save_settings(&s);
    }

    fn persist_key(&self, config: &str, key: &str, value: &str) {
        let mut s = self.settings.borrow_mut();
        s.set_config_key(config, key, value);
        self.save_settings(&s);
    }

    fn save_settings(&self, s: &Settings) {
        if let Err(e) = s.save() {
            log::warn!("cannot save settings: {e}");
        }
    }

    // ------------------------------------------------------------ requests

    /// Queue requests from a skin; they run after the caller returns.
    pub fn submit(self: &Rc<Self>, from: &str, reqs: Vec<AppRequest>) {
        self.queue.borrow_mut().extend(reqs.into_iter().map(|r| (from.to_string(), r)));
        if self.draining.replace(true) {
            return;
        }
        let mut budget = 2000;
        loop {
            let next = self.queue.borrow_mut().pop_front();
            let Some((from, req)) = next else { break };
            budget -= 1;
            if budget == 0 {
                log::warn!("request loop limit reached, dropping {} queued requests", self.queue.borrow().len());
                self.queue.borrow_mut().clear();
                break;
            }
            self.handle(&from, req);
        }
        self.draining.set(false);
    }

    fn targets(&self, config: &str) -> Vec<Rc<SkinWindow>> {
        if config == "*" {
            return self.all();
        }
        if let Some(g) = config.strip_prefix("Group=") {
            return self
                .all()
                .into_iter()
                .filter(|w| w.skin.borrow().groups().iter().any(|x| x.eq_ignore_ascii_case(g)))
                .collect();
        }
        self.find(config).into_iter().collect()
    }

    fn handle(self: &Rc<Self>, _from: &str, req: AppRequest) {
        match req {
            AppRequest::Activate { config, file } => {
                if let Err(e) = self.activate(&config, file.as_deref()) {
                    log::warn!("cannot activate {config}: {e}");
                }
            }
            AppRequest::Deactivate { config } => {
                for w in self.targets(&config) {
                    self.unload(&w.config.clone());
                }
            }
            AppRequest::Toggle { config, file } => {
                if self.find(&config).is_some() {
                    self.unload(&config);
                } else if let Err(e) = self.activate(&config, file.as_deref()) {
                    log::warn!("cannot activate {config}: {e}");
                }
            }
            AppRequest::Refresh { config } => match config {
                None => self.all().iter().for_each(|w| self.reload(w)),
                Some(c) => self.targets(&c).iter().for_each(|w| self.reload(w)),
            },
            AppRequest::RefreshApp => {
                self.rescan();
                self.all().iter().for_each(|w| self.reload(w));
            }
            AppRequest::Move { config, x, y } => {
                for w in self.targets(&config) {
                    w.move_to(x, y);
                    if w.state().save_position {
                        let (a, b) = w.position_text();
                        self.persist_position(&w.config, &a, &b);
                    }
                }
            }
            AppRequest::SetDraggable { config, value } => {
                for w in self.targets(&config) {
                    let v = tri(value, w.state().draggable);
                    self.set_flag(&w, "draggable", v);
                }
            }
            AppRequest::SetClickThrough { config, value } => {
                for w in self.targets(&config) {
                    let v = tri(value, w.state().click_through);
                    self.set_flag(&w, "clickthrough", v);
                }
            }
            AppRequest::SetTransparency { config, alpha } => {
                for w in self.targets(&config) {
                    w.set_alpha(alpha);
                    self.persist_key(&w.config, "AlphaValue", &alpha.to_string());
                }
            }
            AppRequest::ZPos { config, value } => {
                for w in self.targets(&config) {
                    w.set_zpos(value);
                    self.persist_key(&w.config, "AlwaysOnTop", &value.to_string());
                }
            }
            AppRequest::Show { config } => self.targets(&config).iter().for_each(|w| w.set_visible(true)),
            AppRequest::Hide { config } => self.targets(&config).iter().for_each(|w| w.set_visible(false)),
            AppRequest::ToggleVisible { config } => self.targets(&config).iter().for_each(|w| w.set_visible(!w.is_shown())),
            AppRequest::BangOther { config, actions } => {
                for w in self.targets(&config) {
                    w.execute(&actions);
                }
            }
            AppRequest::OpenOrRun(s) => open_or_run(&s),
            AppRequest::EmptyTrash { confirm } => {
                if confirm {
                    let dialog = gtk4::AlertDialog::builder()
                        .message("Empty the Trash?")
                        .detail("All items in the Trash will be permanently deleted.")
                        .buttons(["Cancel", "Empty Trash"])
                        .cancel_button(0)
                        .default_button(0)
                        .modal(true)
                        .build();
                    dialog.choose(None::<&gtk4::Window>, None::<&gio::Cancellable>, |res| {
                        if matches!(res, Ok(1)) {
                            empty_trash();
                        }
                    });
                } else {
                    empty_trash();
                }
            }
            AppRequest::Resize => {}
            AppRequest::Quit => self.gapp.quit(),
            AppRequest::InputText { config, spec } => {
                if let Some(w) = self.find(&config) {
                    let weak = Rc::downgrade(&w);
                    let commands = spec.commands.clone();
                    inputtext::show(&spec, w.position(), move |text| {
                        if let Some(w) = weak.upgrade() {
                            for c in inputtext::expand_commands(&commands, &text) {
                                w.execute(&c);
                            }
                        }
                    });
                }
            }
        }
    }

    fn set_flag(&self, w: &Rc<SkinWindow>, which: &str, v: bool) {
        let (key, flag) = match which {
            "draggable" => {
                w.set_draggable(v);
                ("Draggable", v)
            }
            "clickthrough" => {
                w.set_click_through(v);
                ("ClickThrough", v)
            }
            _ => {
                w.set_keep_on_screen(v);
                ("KeepOnScreen", v)
            }
        };
        self.persist_key(&w.config, key, if flag { "1" } else { "0" });
    }

    /// Context-menu toggles.
    pub fn menu_toggle(&self, config: &str, which: &str, on: bool) {
        if let Some(w) = self.find(config) {
            self.set_flag(&w, which, on);
        }
    }

    // ------------------------------------------------------------ loading

    fn resolve(&self, config: &str, file: Option<&str>) -> Result<(String, String), String> {
        let mut reg = self.registry.borrow_mut();
        if reg.config(config).is_none() {
            *reg = SkinRegistry::scan(&self.env.borrow());
        }
        let c = reg.config(&norm(config)).ok_or_else(|| format!("config \"{config}\" not found"))?;
        let file = match file {
            Some(f) => reg.file_index(&c.name, f).and_then(|i| reg.file_at(&c.name, i)).map(str::to_string).ok_or_else(|| format!("\"{f}\" not found in {}", c.name))?,
            None => {
                let from_settings = self.settings.borrow().config(&c.name).filter(|s| s.active > 0).and_then(|s| reg.file_at(&c.name, s.active).map(str::to_string));
                from_settings.or_else(|| c.files.first().cloned()).ok_or_else(|| format!("{} has no .ini files", c.name))?
            }
        };
        Ok((c.name.clone(), file))
    }

    /// Mark active, save and open the window.
    pub fn activate(self: &Rc<Self>, config: &str, file: Option<&str>) -> Result<String, String> {
        let (name, file) = self.resolve(config, file)?;
        if let Some(w) = self.find(&name) {
            if w.file.borrow().eq_ignore_ascii_case(&file) {
                return Ok(format!("{name} is already loaded"));
            }
            self.unload(&name);
        }
        {
            let mut s = self.settings.borrow_mut();
            activate_skin(&self.env.borrow(), &mut s, &name, &file)?;
            self.save_settings(&s);
        }
        self.open_window(&name, &file)?;
        self.update_tray();
        Ok(format!("{name} [{file}] loaded"))
    }

    fn open_window(self: &Rc<Self>, name: &str, file: &str) -> Result<(), String> {
        let skin = Skin::load(&self.env.borrow(), name, file)?;
        let (cfg, anchor, hidden) = {
            let s = self.settings.borrow();
            (
                s.config(name).unwrap_or_else(ConfigState::default),
                (s.config_key(name, "AnchorX").unwrap_or("0").to_string(), s.config_key(name, "AnchorY").unwrap_or("0").to_string()),
                s.config_key(name, "StartHidden").map(|v| v.trim() == "1").unwrap_or(false),
            )
        };
        let w = SkinWindow::create(self, name, file, skin, cfg, anchor, hidden);
        self.windows.borrow_mut().push(w);
        Ok(())
    }

    pub fn unload(self: &Rc<Self>, config: &str) -> bool {
        let Some(w) = self.find(config) else { return false };
        self.close_window(&w, true);
        true
    }

    fn close_window(self: &Rc<Self>, w: &Rc<SkinWindow>, mark_inactive: bool) {
        let on_close = w.skin.borrow().options().on_close.clone();
        if !on_close.trim().is_empty() {
            w.skin.borrow_mut().execute(&on_close);
            // Requests raised by OnCloseAction still run (e.g. other skins).
            let reqs = w.skin.borrow_mut().take_requests();
            if !reqs.is_empty() {
                self.submit(&w.config, reqs);
            }
        }
        self.windows.borrow_mut().retain(|x| !Rc::ptr_eq(x, w));
        w.destroy();
        if mark_inactive {
            let mut s = self.settings.borrow_mut();
            s.set_active(&w.config, 0);
            self.save_settings(&s);
        }
        self.update_tray();
    }

    fn reload(self: &Rc<Self>, w: &Rc<SkinWindow>) {
        let file = w.file.borrow().clone();
        match Skin::load(&self.env.borrow(), &w.config, &file) {
            Ok(skin) => w.replace_skin(skin),
            Err(e) => log::warn!("refresh of {} failed: {e}", w.config),
        }
    }

    /// Re-read the skins folder and `snowmeter.ini`; open/close windows to match.
    fn rescan(self: &Rc<Self>) {
        let env = self.env.borrow().clone();
        *self.registry.borrow_mut() = SkinRegistry::scan(&env);
        *self.settings.borrow_mut() = Settings::load(&env);
        let reg = self.registry.borrow().clone();
        let active = active_from_settings(&reg, &self.settings.borrow());
        for w in self.all() {
            if !active.iter().any(|(n, _)| n.eq_ignore_ascii_case(&w.config)) {
                self.close_window(&w, false);
            }
        }
        for (name, file) in active {
            if self.find(&name).is_none() {
                if let Err(e) = self.open_window(&name, &file) {
                    log::warn!("cannot load {name}: {e}");
                }
            }
        }
        self.update_tray();
    }

    fn tray_list(&self) -> Vec<(String, bool)> {
        self.registry
            .borrow()
            .configs
            .iter()
            .filter(|c| !c.files.is_empty())
            .map(|c| (c.name.clone(), self.find(&c.name).is_some()))
            .collect()
    }

    /// Names of gallery entries that are not installed yet.
    fn gallery_list(&self) -> Vec<String> {
        let Some(dir) = crate::firstrun::bundled_dir() else { return Vec::new() };
        let env = self.env.borrow();
        gallery::load_catalog(&dir).into_iter().filter(|e| !gallery::is_installed(&env, e)).map(|e| e.name).collect()
    }

    /// Install a gallery entry on a worker thread (downloads can be slow).
    fn install_gallery(self: &Rc<Self>, name: String) {
        if self.installing.borrow().contains(&name) {
            return;
        }
        let Some(dir) = crate::firstrun::bundled_dir() else {
            log::warn!("gallery: bundled skins folder not found");
            return;
        };
        let Some(entry) = gallery::load_catalog(&dir).into_iter().find(|e| e.name == name) else {
            log::warn!("gallery: no catalog entry \"{name}\"");
            return;
        };
        self.installing.borrow_mut().push(name.clone());
        log::info!("gallery: installing {name}");
        let env = self.env.borrow().clone();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let mut settings = Settings::load(&env);
            let opts = gallery::InstallOptions { bundled_dir: Some(dir), ..Default::default() };
            let res = gallery::install(&env, &mut settings, &entry, &opts).map(|r| r.to_string()).map_err(|e| e.to_string());
            let _ = tx.send(Msg::GalleryDone(name, res));
        });
    }

    /// Tell the ConfigActive plugin which configs exist and which are loaded.
    fn publish_active(&self) {
        let known: Vec<String> = self.registry.borrow().configs.iter().map(|c| c.name.clone()).collect();
        let active: Vec<String> = self.windows.borrow().iter().map(|w| w.config.clone()).collect();
        snowmeter_core::active::publish(known, active);
    }

    fn update_tray(&self) {
        self.publish_active();
        if let Some(t) = self.tray.borrow().as_ref() {
            t.set_skins(self.tray_list(), self.gallery_list());
        }
    }

    // ------------------------------------------------------------ messages

    fn handle_msg(self: &Rc<Self>, m: Msg) {
        match m {
            Msg::Ipc(req, reply) => {
                let (text, quit) = self.handle_ipc(req);
                let _ = reply.send(text);
                if quit {
                    self.gapp.quit();
                }
            }
            Msg::Tray(cmd) => match cmd {
                TrayCmd::RefreshAll => self.submit("tray", vec![AppRequest::RefreshApp]),
                TrayCmd::ToggleSkin(c) => self.submit("tray", vec![AppRequest::Toggle { config: c, file: None }]),
                TrayCmd::OpenSkinsFolder => open_or_run(&self.env.borrow().skins_dir.to_string_lossy()),
                TrayCmd::InstallSkin(n) => self.install_gallery(n),
                TrayCmd::Quit => self.gapp.quit(),
            },
            Msg::GalleryDone(name, res) => {
                self.installing.borrow_mut().retain(|n| *n != name);
                match res {
                    Ok(report) => log::info!("gallery: installed {name}\n{report}"),
                    Err(e) => log::warn!("gallery: installing {name} failed: {e}"),
                }
                self.rescan();
            }
        }
    }

    fn handle_ipc(self: &Rc<Self>, req: Request) -> (String, bool) {
        match req {
            Request::Ping => (ipc::ok("pong"), false),
            Request::Load { config, file } => match self.activate(&config, file.as_deref()) {
                Ok(m) => (ipc::ok(&m), false),
                Err(e) => (ipc::err(&e), false),
            },
            Request::Unload { config } => {
                if self.unload(&config) {
                    (ipc::ok(&format!("{config} unloaded")), false)
                } else {
                    (ipc::err(&format!("\"{config}\" is not loaded")), false)
                }
            }
            Request::List => {
                let reg = SkinRegistry::scan(&self.env.borrow());
                let active: Vec<(String, String)> =
                    self.all().iter().map(|w| (w.config.clone(), w.file.borrow().clone())).collect();
                (format!("ok\n{}", format_list(&reg, &active)), false)
            }
            Request::Refresh { config } => match config {
                Some(c) if self.find(&c).is_none() => (ipc::err(&format!("\"{c}\" is not loaded")), false),
                Some(c) => {
                    self.submit("ipc", vec![AppRequest::Refresh { config: Some(c) }]);
                    (ipc::ok("refreshed"), false)
                }
                None => {
                    self.submit("ipc", vec![AppRequest::Refresh { config: None }]);
                    (ipc::ok("refreshed all"), false)
                }
            },
            Request::Rescan => {
                self.rescan();
                (ipc::ok("rescanned"), false)
            }
            Request::Quit => (ipc::ok("quitting"), true),
        }
    }

    fn shutdown(self: &Rc<Self>) {
        for w in self.all() {
            self.close_window(&w, false);
        }
        if let Some(t) = self.tray.borrow_mut().take() {
            t.shutdown();
        }
        let _ = std::fs::remove_file(&self.sock);
        log::info!("snowmeter stopped");
    }
}

fn tri(value: i32, current: bool) -> bool {
    match value {
        -1 => !current,
        0 => false,
        _ => true,
    }
}

/// Start the application. Returns the process exit code.
pub fn run() -> i32 {
    if let Err(e) = gtk4::init() {
        eprintln!("snowmeter: cannot open a display: {e}");
        return 1;
    }
    let sock = ipc::socket_path();
    let listener = match ipc::bind(&sock) {
        Ok(ipc::Bind::AlreadyRunning) => {
            println!("snowmeter is already running");
            return 0;
        }
        Ok(ipc::Bind::Listener(l)) => l,
        Err(e) => {
            eprintln!("snowmeter: cannot create socket {}: {e}", sock.display());
            return 1;
        }
    };
    let (tx, rx) = mpsc::channel::<Msg>();
    {
        let tx = tx.clone();
        ipc::serve(listener, move |req| {
            let (rtx, rrx) = mpsc::channel();
            if tx.send(Msg::Ipc(req, rtx)).is_err() {
                return ipc::err("shutting down");
            }
            rrx.recv_timeout(Duration::from_secs(8)).unwrap_or_else(|_| ipc::err("timed out"))
        });
    }

    let gapp = gtk4::Application::builder().application_id("org.snowmeter.Snowmeter").flags(gio::ApplicationFlags::NON_UNIQUE).build();
    let rx = Rc::new(RefCell::new(Some(rx)));
    let tx_tray = tx.clone();
    gapp.connect_activate(move |gapp| {
        let Some(rx) = rx.borrow_mut().take() else { return };
        let inner = start(gapp, sock.clone(), &tx_tray);
        let poll = inner.clone();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            while let Ok(m) = rx.try_recv() {
                poll.handle_msg(m);
            }
            glib::ControlFlow::Continue
        });
        let shut = inner.clone();
        gapp.connect_shutdown(move |_| shut.shutdown());
        std::mem::forget(inner);
    });
    let code = gapp.run_with_args::<&str>(&[]);
    i32::from(code)
}

fn start(gapp: &gtk4::Application, sock: std::path::PathBuf, tx: &mpsc::Sender<Msg>) -> Rc<Inner> {
    window::install_css();
    // Keep the application alive with zero windows.
    std::mem::forget(gapp.hold());
    let mut env = Environment::from_env();
    let monitors = read_monitors();
    env.monitors = monitors.clone();
    env.work_areas = monitors;
    let _ = std::fs::create_dir_all(&env.skins_dir);
    let mut settings = Settings::load(&env);
    firstrun::run(&env, &mut settings);
    let registry = SkinRegistry::scan(&env);
    if !x11::is_x11() {
        log::warn!("not an X11 session: window type and position hints are unavailable; for desktop-layer windows run with GDK_BACKEND=x11");
        #[cfg(feature = "wayland-layer-shell")]
        log::warn!("feature wayland-layer-shell is enabled but gtk4-layer-shell is not linked in this build");
    }
    let inner = Rc::new(Inner {
        gapp: gapp.clone(),
        window_type: Cell::new(settings.window_type()),
        env: RefCell::new(env),
        settings: RefCell::new(settings),
        registry: RefCell::new(registry),
        windows: RefCell::new(Vec::new()),
        queue: RefCell::new(VecDeque::new()),
        draining: Cell::new(false),
        tray: RefCell::new(None),
        sock,
        tx: tx.clone(),
        installing: RefCell::new(Vec::new()),
    });
    if let Some(display) = gdk::Display::default() {
        let weak = Rc::downgrade(&inner);
        display.monitors().connect_items_changed(move |_, _, _, _| {
            if let Some(i) = weak.upgrade() {
                i.refresh_monitors();
                i.all().iter().for_each(|w| w.reposition());
            }
        });
    }
    let reg = inner.registry.borrow().clone();
    for (name, file) in active_from_settings(&reg, &inner.settings.borrow()) {
        if let Err(e) = inner.open_window(&name, &file) {
            log::warn!("cannot load {name}: {e}");
        }
    }
    inner.publish_active();
    let tx = tx.clone();
    let tray = Tray::start(inner.tray_list(), inner.gallery_list(), move |cmd| {
        let _ = tx.send(Msg::Tray(cmd));
    });
    *inner.tray.borrow_mut() = tray;
    log::info!("snowmeter started ({} skins)", inner.windows.borrow().len());
    inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words() {
        assert_eq!(split_words("a b  c"), vec!["a", "b", "c"]);
        assert_eq!(split_words("\"my prog\" -x \"a b\""), vec!["my prog", "-x", "a b"]);
        assert_eq!(split_words(""), Vec::<String>::new());
        assert_eq!(split_words("a \"\" b"), vec!["a", "", "b"]);
    }

    #[test]
    fn classify() {
        assert_eq!(classify_command("http://example.com/x"), Launch::Open("http://example.com/x".into()));
        assert_eq!(classify_command("\"https://a.b\""), Launch::Open("https://a.b".into()));
        assert_eq!(classify_command("mailto:a@b.c"), Launch::Open("mailto:a@b.c".into()));
        let t = tempfile::tempdir().unwrap();
        assert_eq!(classify_command(&t.path().to_string_lossy()), Launch::Open(t.path().to_string_lossy().into_owned()));
        let f = t.path().join("doc.txt");
        std::fs::write(&f, "x").unwrap();
        assert_eq!(classify_command(&format!("\"{}\"", f.display())), Launch::Open(f.to_string_lossy().into_owned()));
        assert_eq!(classify_command("gnome-system-monitor --foo"), Launch::Run(vec!["gnome-system-monitor".into(), "--foo".into()]));
    }

    #[test]
    fn tri_state() {
        assert!(tri(-1, false));
        assert!(!tri(-1, true));
        assert!(!tri(0, true));
        assert!(tri(1, false));
    }
}
