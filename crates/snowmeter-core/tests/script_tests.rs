//! Script (Lua) measure tests, driven through a loaded skin.

use snowmeter_core::env::Environment;
use snowmeter_core::skin::Skin;

struct Fx {
    _tmp: tempfile::TempDir,
    env: Environment,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let env = Environment::with_root(tmp.path());
        std::fs::create_dir_all(&env.skins_dir).unwrap();
        Fx { _tmp: tmp, env }
    }

    fn skin(&self, ini: &str, lua: &str) -> Skin {
        let dir = self.env.skins_dir.join("S");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Main.ini"), ini).unwrap();
        std::fs::write(dir.join("s.lua"), lua).unwrap();
        Skin::load(&self.env, "S", "Main.ini").unwrap()
    }
}

const INI: &str = "[Variables]\nGreeting=hello\n[Other]\nMeasure=Calc\nFormula=7\n\
    [Sc]\nMeasure=Script\nScriptFile=s.lua\nCustom=abc#Greeting#\nNum=(2+3)\n\
    [T]\nMeter=String\nMeasureName=Sc\nText=%1\n";

#[test]
fn update_returning_number_sets_value() {
    let fx = Fx::new();
    let mut s = fx.skin(INI, "function Update() return 42 end");
    s.tick();
    assert_eq!(s.measure_value("Sc"), Some(42.0));
}

#[test]
fn update_returning_string_sets_string_and_numeric_prefix() {
    let fx = Fx::new();
    let mut s = fx.skin(INI, "function Update() return '12 px' end");
    s.tick();
    assert_eq!(s.measure_value("Sc"), Some(12.0));
    assert_eq!(s.meter_text("T").as_deref(), Some("12 px"));
    let mut s = fx.skin(INI, "function Update() return 3, 'x' end");
    s.tick();
    assert_eq!(s.measure_value("Sc"), Some(3.0));
    assert_eq!(s.meter_text("T").as_deref(), Some("x"));
}

#[test]
fn initialize_runs_once_before_update() {
    let fx = Fx::new();
    let mut s = fx.skin(INI, "n = 0\nfunction Initialize() n = 100 end\nfunction Update() n = n + 1 return n end");
    // Loading runs the forced first update.
    assert_eq!(s.measure_value("Sc"), Some(101.0));
    s.tick();
    assert_eq!(s.measure_value("Sc"), Some(102.0));
}

#[test]
fn skin_api_variables_options_and_measures() {
    let fx = Fx::new();
    let lua = "function Update()\n\
        local v = SKIN:GetVariable('Greeting')\n\
        if v ~= 'hello' then return -1 end\n\
        if SKIN:GetVariable('nope', 'dflt') ~= 'dflt' then return -2 end\n\
        if SELF:GetOption('Custom') ~= 'abchello' then return -3 end\n\
        if SELF:GetNumberOption('Num') ~= 5 then return -4 end\n\
        if SELF:GetName() ~= 'Sc' then return -5 end\n\
        local m = SKIN:GetMeasure('Other')\n\
        if m == nil then return -6 end\n\
        if SKIN:GetMeasure('missing') ~= nil then return -7 end\n\
        if SKIN:ParseFormula('(3*4)') ~= 12 then return -8 end\n\
        return m:GetValue() + 1000 * #m:GetStringValue()\n\
        end";
    let mut s = fx.skin(INI, lua);
    s.tick();
    assert_eq!(s.measure_value("Sc"), Some(1007.0));
}

#[test]
fn bangs_from_lua_run_after_the_call() {
    let fx = Fx::new();
    let lua = "function Update()\n SKIN:Bang('!SetVariable', 'Greeting', 'bye bye')\n SKIN:Bang('!SetVariable Other 2')\n return 1 end";
    let mut s = fx.skin(INI, lua);
    s.tick();
    assert_eq!(s.parser().get_variable("Greeting").as_deref(), Some("bye bye"));
    assert_eq!(s.parser().get_variable("Other").as_deref(), Some("2"));
}

#[test]
fn runtime_error_is_logged_and_value_unchanged() {
    let fx = Fx::new();
    let mut s = fx.skin(INI, "n = 0\nfunction Update() n = n + 1 if n == 2 then error('boom') end return n end");
    assert_eq!(s.measure_value("Sc"), Some(1.0));
    s.tick();
    assert_eq!(s.measure_value("Sc"), Some(1.0), "failed update keeps the value");
    assert!(s.log_messages().iter().any(|m| m.contains("Update() failed") && m.contains("boom")), "{:?}", s.log_messages());
    s.tick();
    assert_eq!(s.measure_value("Sc"), Some(3.0));
}

#[test]
fn syntax_error_is_logged_value_zero() {
    let fx = Fx::new();
    let mut s = fx.skin(INI, "function Update( return 1");
    s.tick();
    assert_eq!(s.measure_value("Sc"), Some(0.0));
    assert!(s.log_messages().iter().any(|m| m.contains("File not valid")));
}

#[test]
fn command_measure_runs_a_chunk_in_the_script_environment() {
    let fx = Fx::new();
    let mut s = fx.skin(INI, "x = 5\nfunction Update() return x end");
    s.tick();
    s.execute("!CommandMeasure Sc \"x = 9\"");
    s.tick();
    assert_eq!(s.measure_value("Sc"), Some(9.0));
}

#[test]
fn print_goes_to_the_log_and_unsafe_libs_are_gone() {
    let fx = Fx::new();
    let lua = "function Update()\n print('a', 1)\n if io.popen or os.exit or require or collectgarbage then return -1 end\n return bit.band(6, 3) end";
    let mut s = fx.skin(INI, lua);
    s.tick();
    assert_eq!(s.measure_value("Sc"), Some(2.0));
    assert!(s.log_messages().iter().any(|m| m.contains("a    1")));
}
