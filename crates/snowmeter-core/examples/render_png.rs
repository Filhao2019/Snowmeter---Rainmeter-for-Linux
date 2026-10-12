//! `render_png <skins-root> <config> <out.png>`: headless render of one skin.
use snowmeter_core::env::Environment;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: render_png <root> <config> <out.png>");
        std::process::exit(2);
    }
    let env = Environment::with_root(Path::new(&a[1]));
    if let Err(e) = snowmeter_core::render_skin_png(&env, &a[2], None, 1, Path::new(&a[3])) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
