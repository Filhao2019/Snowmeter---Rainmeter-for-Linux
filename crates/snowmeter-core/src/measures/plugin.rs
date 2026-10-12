//! Measure factory, `Measure=Plugin` redirection and the placeholder used for
//! Windows DLL plugins that cannot run on Linux.

use super::calc::CalcMeasure;
use super::cpu::CpuMeasure;
use super::disk::DiskMeasure;
use super::loop_::LoopMeasure;
use super::memory::{MemoryKind, MemoryMeasure};
use super::net::{NetKind, NetMeasure};
use super::registry::RegistryMeasure;
use super::string::StringMeasure;
use super::time::TimeMeasure;
use super::uptime::UptimeMeasure;
use super::{MeasureCtx, MeasureImpl};
use crate::parser::OptionReader;

/// Measure with no behaviour: value 0, no string, commands ignored. Logs one
/// line at load.
pub struct Placeholder {
    msg: Option<String>,
}

impl Placeholder {
    pub fn new(msg: &str) -> Placeholder {
        Placeholder { msg: Some(msg.to_string()) }
    }
}

impl MeasureImpl for Placeholder {
    fn read_options(&mut self, _opt: &OptionReader) {}
    fn update(&mut self, _ctx: &mut MeasureCtx) -> f64 {
        0.0
    }
    fn take_log(&mut self) -> Vec<String> {
        self.msg.take().into_iter().collect()
    }
}

type Ctor = fn() -> Box<dyn MeasureImpl>;

/// Native Linux re-implementations of Rainmeter plugins, keyed by lowercase
/// canonical name. Empty for now; later milestones add SysInfo, RecycleManager,
/// Power, WebParser and Script here.
const NATIVE_PLUGINS: &[(&str, Ctor)] = &[("script", || Box::new(super::script::ScriptMeasure::new())),
    ("webparser", || Box::new(super::webparser::WebParser::new())),
    ("sysinfo", || Box::new(super::sysinfo::SysInfo::new())),
    ("recyclemanager", || Box::new(super::recycle::RecycleMeasure::new())),
    ("power", || Box::new(super::power::PowerMeasure::new())),
    ("nowplaying", || Box::new(super::nowplaying::NowPlayingMeasure::new())),
    ("inputtext", || Box::new(super::inputtext::InputTextMeasure::new())),
    ("webnowplaying", || Box::new(super::nowplaying::web::WebNowPlayingMeasure::new())),
    ("process", || Box::new(super::process::ProcessMeasure::new())),
    ("usagemonitor", || Box::new(super::usagemonitor::UsageMonitor::new())),
    ("folderinfo", || Box::new(super::folderinfo::FolderInfo::new())),
    ("fileview", || Box::new(super::fileview::FileView::new())),
    ("actiontimer", || Box::new(super::actiontimer::ActionTimer::new())),
    ("configactive", || Box::new(super::configactive::ConfigActive::new())),
    ("runcommand", || Box::new(super::runcommand::RunCommand::new())),
    ("mouse", || Box::new(super::mouse::MouseMeasure::new())),
    ("coretemp", || Box::new(super::temps::CoreTemp::new())),
    ("speedfan", || Box::new(super::temps::SpeedFan::new())),
    ("hwinfo", || Box::new(super::temps::HwInfo::new())),
    ("ping", || Box::new(super::ping::Ping::new())),
    ("audiolevel", || Box::new(super::audiolevel::AudioLevel::new())),
    ("wifistatus", || Box::new(super::wifi::WifiStatus::new())),
    ("easyblur", || Box::new(super::cosmetic::CosmeticMeasure::new(super::cosmetic::Flavor::Blur))),
    ("frostedglass", || Box::new(super::cosmetic::CosmeticMeasure::new(super::cosmetic::Flavor::Blur))),
    ("chameleon", || Box::new(super::cosmetic::CosmeticMeasure::new(super::cosmetic::Flavor::Chameleon))),
    ("syscolor", || Box::new(super::cosmetic::CosmeticMeasure::new(super::cosmetic::Flavor::SysColor))),
    ("meterialyou", || Box::new(super::cosmetic::CosmeticMeasure::new(super::cosmetic::Flavor::MeterialYou))),
];

/// Names Rainmeter treats as internal measures / default plugins, with the DLL
/// file names that redirect to them. (lowercase alias, canonical lowercase name)
const INTERNAL_NAMES: &[(&str, &str)] = &[
    ("webparser", "webparser"),
    ("nowplaying", "nowplaying"),
    ("webnowplaying", "webnowplaying"),
    ("gpmdpplugin", "webnowplaying"),
    ("gpmdp", "webnowplaying"),
    ("configactive", "configactive"),
    ("easyblur", "easyblur"),
    ("frostedglass", "frostedglass"),
    ("chameleon", "chameleon"),
    ("syscolor", "syscolor"),
    ("meterialyou", "meterialyou"),
    ("materialyou", "meterialyou"),
    ("sysinfo", "sysinfo"),
    ("process", "process"),
    ("recyclemanager", "recyclemanager"),
    ("wifistatus", "wifistatus"),
    ("mediakey", "mediakey"),
    ("actiontimer", "actiontimer"),
    ("advancedcpu", "advancedcpu"),
    ("audiolevel", "audiolevel"),
    ("audiolevelbeta", "audiolevel"),
    ("coretemp", "coretemp"),
    ("hwinfo", "hwinfo"),
    ("drag&drop", "dragdrop"),
    ("dragdrop", "dragdrop"),
    ("fileview", "fileview"),
    ("folderinfo", "folderinfo"),
    ("inputtext", "inputtext"),
    ("itunesplugin", "itunes"),
    ("itunes", "itunes"),
    ("mouse", "mouse"),
    ("perfmon", "perfmon"),
    ("pingplugin", "ping"),
    ("ping", "ping"),
    ("powerplugin", "power"),
    ("power", "power"),
    ("quoteplugin", "quote"),
    ("quote", "quote"),
    ("resmon", "resmon"),
    ("runcommand", "runcommand"),
    ("speedfanplugin", "speedfan"),
    ("speedfan", "speedfan"),
    ("usagemonitor", "usagemonitor"),
    ("win7audioplugin", "audio"),
    ("audio", "audio"),
    ("windowmessageplugin", "windowmessage"),
    ("windowmessage", "windowmessage"),
    ("script", "script"),
];

/// Strip directories and a `.dll` suffix, lowercase.
fn plugin_key(name: &str) -> String {
    let file = name.rsplit(['\\', '/']).next().unwrap_or(name).trim();
    let lower = file.to_ascii_lowercase();
    lower.strip_suffix(".dll").unwrap_or(&lower).to_string()
}

/// Canonical (lowercase) internal name for a measure/plugin name, if any.
pub fn canonical_plugin_name(name: &str) -> Option<&'static str> {
    let key = plugin_key(name);
    INTERNAL_NAMES.iter().find(|(alias, _)| *alias == key).map(|(_, canon)| *canon)
}

/// Names of the plugins Snowmeter implements natively (lowercase canonical).
pub fn native_plugin_names() -> Vec<&'static str> {
    NATIVE_PLUGINS.iter().map(|(n, _)| *n).collect()
}

/// True if the plugin (any alias / `.dll` spelling) has a native implementation.
pub fn is_native_plugin(name: &str) -> bool {
    let key = plugin_key(name);
    let canon = canonical_plugin_name(&key).unwrap_or("");
    NATIVE_PLUGINS.iter().any(|(n, _)| *n == key || *n == canon)
}

/// `Measure=` types handled by the factory itself (`Plugin` included).
pub const BUILTIN_MEASURE_TYPES: [&str; 15] = [
    "cpu", "physicalmemory", "swapmemory", "memory", "freediskspace", "uptime", "time", "calc", "string", "netin",
    "netout", "nettotal", "registry", "loop", "plugin",
];

/// True if `Measure=<name>` is a built-in type or a known plugin alias
/// (`Measure=NowPlaying` style); the latter may still be a placeholder.
pub fn is_known_measure_type(name: &str) -> bool {
    let t = name.trim().to_ascii_lowercase();
    BUILTIN_MEASURE_TYPES.contains(&t.as_str()) || canonical_plugin_name(&t).is_some()
}

/// Native implementation of a plugin by name (`.dll` suffix and case ignored).
pub fn native_plugin(name: &str) -> Option<Box<dyn MeasureImpl>> {
    let key = plugin_key(name);
    let canon = canonical_plugin_name(&key).unwrap_or("");
    NATIVE_PLUGINS.iter().find(|(n, _)| *n == key || *n == canon).map(|(_, ctor)| ctor())
}

/// Build the implementation for a `Measure=` value. The bool is false when the
/// type is unknown (the skin should not create a section object then).
pub fn create_measure_impl(
    type_name: &str,
    section: &str,
    opt: &OptionReader,
    log: &mut Vec<String>,
) -> (Box<dyn MeasureImpl>, bool) {
    let t = type_name.trim().to_ascii_lowercase();
    let imp: Box<dyn MeasureImpl> = match t.as_str() {
        "cpu" => Box::new(CpuMeasure::new()),
        "physicalmemory" => Box::new(MemoryMeasure::new(MemoryKind::Physical)),
        "swapmemory" => Box::new(MemoryMeasure::new(MemoryKind::Swap)),
        "memory" => Box::new(MemoryMeasure::new(MemoryKind::Both)),
        "freediskspace" => Box::new(DiskMeasure::new()),
        "uptime" => Box::new(UptimeMeasure::new()),
        "time" => Box::new(TimeMeasure::new()),
        "calc" => Box::new(CalcMeasure::new()),
        "string" => Box::new(StringMeasure::new()),
        "netin" => Box::new(NetMeasure::new(NetKind::In)),
        "netout" => Box::new(NetMeasure::new(NetKind::Out)),
        "nettotal" => Box::new(NetMeasure::new(NetKind::Total)),
        "registry" => Box::new(RegistryMeasure::new()),
        "loop" => Box::new(LoopMeasure::new()),
        "plugin" => {
            let dll = opt.string("Plugin", "");
            match canonical_plugin_name(&dll) {
                Some(canon) => native_plugin(canon)
                    .unwrap_or_else(|| Box::new(Placeholder::new(&format!("{canon}: not supported on Linux")))),
                None => Box::new(Placeholder::new(&format!("Plugin: Unable to load \"{dll}\" (not supported on Linux)"))),
            }
        }
        other => match canonical_plugin_name(other) {
            Some(canon) => {
                native_plugin(canon).unwrap_or_else(|| Box::new(Placeholder::new(&format!("{canon}: not supported on Linux"))))
            }
            None => {
                log.push(format!("Measure={type_name} is not valid in [{section}]"));
                return (Box::new(Placeholder { msg: None }), false);
            }
        },
    };
    (imp, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measures::testutil::skin;
    use crate::measures::Measure;

    #[test]
    fn names_are_canonicalised() {
        assert_eq!(canonical_plugin_name("WebParser"), Some("webparser"));
        assert_eq!(canonical_plugin_name("Plugins\\PowerPlugin.dll"), Some("power"));
        assert_eq!(canonical_plugin_name("pingplugin.DLL"), Some("ping"));
        assert_eq!(canonical_plugin_name("Win7AudioPlugin"), Some("audio"));
        assert_eq!(canonical_plugin_name("Drag&Drop.dll"), Some("dragdrop"));
        assert_eq!(canonical_plugin_name("HotKey.dll"), None);
    }

    #[test]
    fn builtin_type_list_matches_factory() {
        let s = skin("[M]\nMeasure=X\n");
        for t in BUILTIN_MEASURE_TYPES {
            let opt = OptionReader { parser: &s.parser, section: "M", sv: None };
            let mut log = Vec::new();
            assert!(create_measure_impl(t, "M", &opt, &mut log).1, "{t}");
        }
        let opt = OptionReader { parser: &s.parser, section: "M", sv: None };
        assert!(!create_measure_impl("Bogus", "M", &opt, &mut Vec::new()).1);
        assert!(is_known_measure_type("NowPlaying") && !is_known_measure_type("Bogus"));
        assert!(is_native_plugin("WebParser.dll") && is_native_plugin("PowerPlugin") && is_native_plugin("NowPlaying.dll") && is_native_plugin("Process") && !is_native_plugin("HotKey"));
        assert!(native_plugin_names().contains(&"script"));
    }

    #[test]
    fn native_table_lookup() {
        assert!(native_plugin("Script").is_some());
        assert!(native_plugin("Nope").is_none());
        assert!(native_plugin("Foo.dll").is_none());
    }

    #[test]
    fn unknown_dll_plugin_is_placeholder_with_one_log() {
        let s = skin("[M]\nMeasure=Plugin\nPlugin=HotKey.dll\nMaxValue=5\n");
        let opt = OptionReader { parser: &s.parser, section: "M", sv: None };
        let mut m = Measure::new("M", "Plugin", &opt);
        assert!(m.known_type);
        let log = m.take_log();
        assert_eq!(log.len(), 1);
        assert!(log[0].contains("HotKey.dll"));
        assert!(m.take_log().is_empty());
        assert_eq!(m.value(), 0.0);
        assert_eq!(m.max_value(), 5.0);
        assert!(!m.is_string_measure());
        assert_eq!(m.default_string(), "0");
    }

    #[test]
    fn known_but_unimplemented_plugin_names_are_placeholders() {
        let s = skin("[M]\nMeasure=Plugin\nPlugin=PowerPlugin.dll\n[N]\nMeasure=MediaKey\n");
        let m = Measure::new("M", "Plugin", &OptionReader { parser: &s.parser, section: "M", sv: None });
        assert!(m.known_type);
        let mut n = Measure::new("N", "MediaKey", &OptionReader { parser: &s.parser, section: "N", sv: None });
        assert!(n.known_type);
        assert!(n.take_log()[0].contains("not supported on Linux"));
    }

    #[test]
    fn unknown_measure_type_is_flagged() {
        let s = skin("[M]\nMeasure=Bogus\n");
        let mut m = Measure::new("M", "Bogus", &OptionReader { parser: &s.parser, section: "M", sv: None });
        assert!(!m.known_type);
        assert_eq!(m.take_log(), ["Measure=Bogus is not valid in [M]"]);
    }
}
