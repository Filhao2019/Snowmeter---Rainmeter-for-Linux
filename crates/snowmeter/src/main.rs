//! Snowmeter: run Rainmeter skins on Linux.

mod app;
mod commands;
mod firstrun;
mod inputtext;
mod ipc;
mod logging;
mod menu;
mod position;
mod region;
mod tray;
mod window;
mod x11;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug, PartialEq)]
#[command(name = "snowmeter", version, about = "Run Rainmeter skins on Linux")]
pub struct Cli {
    /// Verbose logging (debug level).
    #[arg(short, long, global = true)]
    pub verbose: bool,
    #[command(subcommand)]
    pub command: Option<Cmd>,
}

#[derive(Subcommand, Debug, PartialEq)]
pub enum Cmd {
    /// Start the application (default when no command is given).
    Start,
    /// Load a skin, e.g. `snowmeter load "Snowmeter\Weekday" [Weekday.ini]`.
    Load { config: String, file: Option<String> },
    /// Unload a skin.
    Unload { config: String },
    /// List skins and whether they are active.
    List,
    /// Refresh one skin, or all skins.
    Refresh { config: Option<String> },
    /// Import a .rmskin package, zip or skin folder.
    Import { path: PathBuf },
    /// List the skin gallery (bundled and downloadable skins).
    Gallery,
    /// Install a gallery skin by name (see `snowmeter gallery`).
    Install {
        name: String,
        /// Reinstall over an existing copy.
        #[arg(long)]
        force: bool,
    },
    /// Compatibility report for a skin, config name or path (no window).
    Check { target: String },
    /// Render a skin to a PNG without a window.
    Render {
        config: String,
        file: Option<String>,
        #[arg(long)]
        out: PathBuf,
        /// Update cycles to run before rendering.
        #[arg(long, default_value_t = 1)]
        ticks: u32,
    },
    /// Run Rainmeter bangs, e.g. `snowmeter bang "[!SetVariable X 1][!Redraw]" "Snowmeter\System"`
    /// (without a config: every loaded skin).
    Bang { actions: String, config: Option<String> },
    /// Resize a loaded skin, e.g. `snowmeter scale "Monterey\Widgets\Clock" 150`
    /// (percent, 25-400; also in the right-click menu and with Ctrl+mouse wheel).
    Scale { config: String, percent: u32 },
    /// Ask the running instance to quit.
    Quit,
}

fn main() {
    let cli = Cli::parse();
    let code = match cli.command.unwrap_or(Cmd::Start) {
        Cmd::Start => {
            logging::init(cli.verbose, true);
            app::run()
        }
        other => {
            logging::init(cli.verbose, false);
            commands::run(other)
        }
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::try_parse_from(std::iter::once("snowmeter").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn default_is_start() {
        assert_eq!(parse(&[]).command, None);
        assert!(parse(&["-v"]).verbose);
        assert_eq!(parse(&["start"]).command, Some(Cmd::Start));
    }

    #[test]
    fn subcommands() {
        assert_eq!(parse(&["load", "illustro\\Clock"]).command, Some(Cmd::Load { config: "illustro\\Clock".into(), file: None }));
        assert_eq!(
            parse(&["load", "a\\b", "x.ini"]).command,
            Some(Cmd::Load { config: "a\\b".into(), file: Some("x.ini".into()) })
        );
        assert_eq!(parse(&["unload", "a"]).command, Some(Cmd::Unload { config: "a".into() }));
        assert_eq!(parse(&["list"]).command, Some(Cmd::List));
        assert_eq!(parse(&["refresh"]).command, Some(Cmd::Refresh { config: None }));
        assert_eq!(parse(&["refresh", "a"]).command, Some(Cmd::Refresh { config: Some("a".into()) }));
        assert_eq!(parse(&["import", "/x.rmskin"]).command, Some(Cmd::Import { path: "/x.rmskin".into() }));
        assert_eq!(parse(&["check", "a"]).command, Some(Cmd::Check { target: "a".into() }));
        assert_eq!(
            parse(&["render", "a", "--out", "o.png", "--ticks", "3"]).command,
            Some(Cmd::Render { config: "a".into(), file: None, out: "o.png".into(), ticks: 3 })
        );
        assert_eq!(
            parse(&["render", "a", "f.ini", "--out", "o.png"]).command,
            Some(Cmd::Render { config: "a".into(), file: Some("f.ini".into()), out: "o.png".into(), ticks: 1 })
        );
        assert_eq!(parse(&["gallery"]).command, Some(Cmd::Gallery));
        assert_eq!(parse(&["install", "Mond"]).command, Some(Cmd::Install { name: "Mond".into(), force: false }));
        assert_eq!(parse(&["install", "Mond", "--force"]).command, Some(Cmd::Install { name: "Mond".into(), force: true }));
        assert_eq!(parse(&["-v", "quit"]).command, Some(Cmd::Quit));
    }

    #[test]
    fn errors() {
        assert!(Cli::try_parse_from(["snowmeter", "load"]).is_err());
        assert!(Cli::try_parse_from(["snowmeter", "render", "a"]).is_err());
        assert!(Cli::try_parse_from(["snowmeter", "nope"]).is_err());
    }
}
