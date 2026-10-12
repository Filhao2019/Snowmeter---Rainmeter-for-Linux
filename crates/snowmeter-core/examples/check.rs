//! `cargo run -p snowmeter-core --example check -- <skins-root> <path-or-config>`
//! Prints one compatibility line per config plus notes.

use snowmeter_core::compat::check_path;
use snowmeter_core::env::Environment;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: check <skins-root> <path-or-config>");
        std::process::exit(2);
    }
    let mut env = Environment::from_env();
    env.skins_dir = std::path::absolute(Path::new(&args[1])).unwrap_or_else(|_| args[1].clone().into());
    match check_path(&env, &args[2]) {
        Ok(reports) => {
            for r in reports {
                println!("{r}");
                for f in &r.files {
                    println!("    file: {f}");
                }
                for n in &r.notes {
                    println!("    note: {n}");
                }
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
