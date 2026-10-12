//! Cairo/Pango rendering helpers shared by all meters.

pub mod fonts;
pub mod image;
pub mod text;

use std::path::PathBuf;

/// Per-skin shared state handed to meters while updating, measuring and drawing.
pub struct Resources {
    pub images: image::ImageCache,
    pub fonts: fonts::FontResolver,
    /// Folder of the skin's .ini (relative image paths start here).
    pub skin_dir: PathBuf,
    /// `[Rainmeter] AccurateText`.
    pub accurate_text: bool,
}

impl Resources {
    pub fn new(skin_dir: PathBuf, accurate_text: bool) -> Resources {
        Resources { images: image::ImageCache::new(), fonts: fonts::FontResolver::new(), skin_dir, accurate_text }
    }
}
