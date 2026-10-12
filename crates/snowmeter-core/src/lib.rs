//! Snowmeter core engine: Rainmeter-compatible INI reading, variables,
//! formulas, colors, paths and bang parsing. No GUI dependencies.

pub mod bangs;
pub mod color;
pub mod env;
pub mod formula;
pub mod ini;
pub(crate) mod numparse;
pub mod parser;
pub(crate) mod subst;
pub mod paths;
pub mod timefmt;
pub mod measures;
pub mod meters;
pub mod render;
pub mod registry;
pub mod rmskin;
pub mod settings;
pub mod skin;

pub use skin::{render_skin_png, AppRequest, MouseButton, MouseEvent, Skin};
