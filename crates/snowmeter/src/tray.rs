//! System tray icon (StatusNotifierItem via ksni). Any failure only logs.

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
use ksni::MenuItem;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayCmd {
    RefreshAll,
    ToggleSkin(String),
    OpenSkinsFolder,
    Quit,
}

pub struct SnowTray {
    /// `(config, active)`.
    pub skins: Vec<(String, bool)>,
    send: Arc<dyn Fn(TrayCmd) + Send + Sync>,
}

impl ksni::Tray for SnowTray {
    fn id(&self) -> String {
        "snowmeter".into()
    }
    fn title(&self) -> String {
        "Snowmeter".into()
    }
    fn icon_name(&self) -> String {
        "preferences-desktop-wallpaper".into()
    }
    fn menu(&self) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        items.push(
            StandardItem {
                label: "Refresh all".into(),
                activate: Box::new(|t: &mut Self| (t.send)(TrayCmd::RefreshAll)),
                ..Default::default()
            }
            .into(),
        );
        let skins: Vec<MenuItem<Self>> = if self.skins.is_empty() {
            vec![StandardItem { label: "(no skins installed)".into(), enabled: false, ..Default::default() }.into()]
        } else {
            self.skins
                .iter()
                .map(|(name, active)| {
                    let n = name.clone();
                    CheckmarkItem {
                        label: name.replace('_', "__"),
                        checked: *active,
                        activate: Box::new(move |t: &mut Self| (t.send)(TrayCmd::ToggleSkin(n.clone()))),
                        ..Default::default()
                    }
                    .into()
                })
                .collect()
        };
        items.push(SubMenu { label: "Skins".into(), submenu: skins, ..Default::default() }.into());
        items.push(
            StandardItem {
                label: "Open skins folder".into(),
                activate: Box::new(|t: &mut Self| (t.send)(TrayCmd::OpenSkinsFolder)),
                ..Default::default()
            }
            .into(),
        );
        items.push(MenuItem::Separator);
        items.push(
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit".into(),
                activate: Box::new(|t: &mut Self| (t.send)(TrayCmd::Quit)),
                ..Default::default()
            }
            .into(),
        );
        items
    }
}

pub struct Tray {
    handle: Handle<SnowTray>,
}

impl Tray {
    /// Start the tray; `None` (after logging) if no tray host is available.
    pub fn start(skins: Vec<(String, bool)>, send: impl Fn(TrayCmd) + Send + Sync + 'static) -> Option<Tray> {
        match (SnowTray { skins, send: Arc::new(send) }).spawn() {
            Ok(handle) => Some(Tray { handle }),
            Err(e) => {
                log::warn!("tray unavailable: {e}");
                None
            }
        }
    }

    pub fn set_skins(&self, skins: Vec<(String, bool)>) {
        let _ = self.handle.update(|t: &mut SnowTray| t.skins = skins);
    }

    pub fn shutdown(&self) {
        let _ = self.handle.shutdown();
    }
}
