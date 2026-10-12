use snowmeter_core::env::Environment;
use snowmeter_core::measures::{Measure, MeasureCtx, MeasureImpl};
use snowmeter_core::parser::{ConfigParser, OptionReader, SkinIdentity};
use std::cell::Cell;
use std::rc::Rc;

struct Skin {
    _tmp: tempfile::TempDir,
    env: Environment,
    p: ConfigParser,
}

fn skin(ini: &str) -> Skin {
    let tmp = tempfile::tempdir().unwrap();
    let env = Environment::with_root(tmp.path());
    let dir = env.skins_dir.join("T");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Main.ini"), ini).unwrap();
    let id = SkinIdentity {
        config: "T".into(),
        root_config: "T".into(),
        file: "Main.ini".into(),
        ini_path: dir.join("Main.ini"),
    };
    let p = ConfigParser::load(&env, &id).unwrap();
    Skin { _tmp: tmp, env, p }
}

impl Skin {
    fn measure(&self, section: &str) -> Measure {
        let ty = self.p.read_string(section, "Measure", "", None);
        Measure::new(section, &ty, &OptionReader { parser: &self.p, section, sv: None })
    }
    fn opt<'a>(&'a self, section: &'a str) -> OptionReader<'a> {
        OptionReader { parser: &self.p, section, sv: None }
    }
    fn tick(&self, m: &mut Measure, counter: u64, lookup: &dyn Fn(&str) -> Option<f64>) -> bool {
        let mut log = Vec::new();
        let dir = self.env.skins_dir.join("T");
        let mut ctx = MeasureCtx { env: &self.env, skin_dir: &dir, update_ms: 1000, log: &mut log, lookup, counter, host: None };
        m.update(&mut ctx, Some(&OptionReader { parser: &self.p, section: &m.name.clone(), sv: None }))
    }
}

fn none(_: &str) -> Option<f64> {
    None
}

/// Calc measure whose value is fed from the outside through the name `Src`.
fn feed(ini_extra: &str) -> (Skin, Measure, Rc<Cell<f64>>) {
    let s = skin(&format!("[M]\nMeasure=Calc\nFormula=Src\n{ini_extra}\n"));
    let m = s.measure("M");
    (s, m, Rc::new(Cell::new(0.0)))
}

fn step(s: &Skin, m: &mut Measure, src: &Rc<Cell<f64>>, v: f64) -> bool {
    src.set(v);
    let c = src.clone();
    let lk = move |n: &str| n.eq_ignore_ascii_case("Src").then(|| c.get());
    s.tick(m, 1, &lk)
}

// ------------------------------------------------------------ section variables

#[test]
fn section_variable_vectors() {
    let s = skin("[Measure]\nMeasure=Calc\nFormula=30\nMinValue=10\nMaxValue=50\n");
    let mut m = s.measure("Measure");
    s.tick(&mut m, 1, &none);
    let sv = |p: &str| m.section_variable(Some(p));
    assert_eq!(m.section_variable(None), "30");
    assert_eq!(sv("%"), "50");
    assert_eq!(sv("%,1"), "50.0");
    assert_eq!(sv(""), "30");
    assert_eq!(sv("2"), "30.00");
    assert_eq!(sv("/5"), "6");
    assert_eq!(sv("/5, 2"), "6.00");
    assert_eq!(sv("MaxValue"), "50");
    assert_eq!(sv("MinValue"), "10");
    assert_eq!(sv("MaxValue:/5"), "10");
    assert_eq!(sv("MaxValue:/5, 2"), "10.00");
    assert_eq!(sv("maxvalue:/5, 2"), "10.00");
    // invalid selectors
    assert_eq!(m.section_variable_opt(Some("MaxValue:%")), None);
    assert_eq!(m.section_variable_opt(Some("/0")), None);
    assert_eq!(m.section_variable_opt(Some("1,2")), None);
    assert_eq!(m.section_variable_opt(Some("Bogus")), None);
}

#[test]
fn section_variable_default_string_trims_zeros() {
    let s = skin("[A]\nMeasure=Calc\nFormula=1/3\n[B]\nMeasure=Calc\nFormula=2.5\n[C]\nMeasure=Calc\nFormula=100\n");
    for (sec, want, two) in [("A", "0.33333", "0.33"), ("B", "2.5", "2.50"), ("C", "100", "100.00")] {
        let mut m = s.measure(sec);
        s.tick(&mut m, 1, &none);
        assert_eq!(m.section_variable(None), want);
        assert_eq!(m.section_variable(Some("2")), two);
    }
}

#[test]
fn string_measure_selectors() {
    let s = skin("[S]\nMeasure=String\nString=\" !*'();:@test&=+$,/?#[ing]\"\nMinValue=12\nMaxValue=34\n");
    let mut m = s.measure("S");
    s.tick(&mut m, 1, &none);
    assert_eq!(
        m.section_variable(Some("EncodeUrl")),
        "%20%21%2A%27%28%29%3B%3A%40test%26%3D%2B%24%2C%2F%3F%23%5Bing%5D"
    );
    assert_eq!(m.section_variable(Some("EscapeRegExp")), " !\\*'\\(\\);:@test&=\\+\\$,/\\?#\\[ing]");
    assert_eq!(m.section_variable(Some("MaxValue")), "34");
    assert_eq!(m.section_variable(Some("MinValue")), "12");
    assert!(m.is_string_measure());
}

#[test]
fn time_timestamp_selector() {
    let s = skin("[T]\nMeasure=Time\nTimeStamp=13354096089\nFormat=%A\n");
    let mut m = s.measure("T");
    s.tick(&mut m, 1, &none);
    assert_eq!(m.section_variable(Some("TimeStamp")), "13354096089");
    assert_eq!(m.section_variable(None), "Tuesday");
}

// ------------------------------------------------------------ formatting

#[test]
fn autoscale_then_substitute() {
    let (s, mut m, src) = feed("Substitute=\" k\":\"KB\"");
    step(&s, &mut m, &src, 1536.0);
    assert_eq!(m.string_value(1, 1.0, 1, false), "1.5KB");
    assert_eq!(m.string_value(0, 1.0, 0, false), "1536");
    assert_eq!(m.string_value(0, 1.0, 1, true), "100.0"); // value is clamped to the default max of 1
}

#[test]
fn percentual_uses_relative_value() {
    let (s, mut m, src) = feed("MaxValue=200");
    step(&s, &mut m, &src, 50.0);
    assert_eq!(m.string_value(0, 1.0, 0, true), "25");
    assert_eq!(m.string_value(0, 1.0, 1, true), "25.0");
    assert_eq!(m.relative_value(), 0.25);
}

#[test]
fn substitute_applies_to_string_values_and_default_string() {
    let s = skin("[S]\nMeasure=String\nString=Tue\nSubstitute=\"Tue\":\"Di\"\n");
    let mut m = s.measure("S");
    s.tick(&mut m, 1, &none);
    assert_eq!(m.string_value(0, 1.0, 0, false), "Di");
    assert_eq!(m.default_string(), "Di");
    // numeric value untouched
    let (s, mut m, src) = feed("Substitute=\"5\":\"five\"");
    step(&s, &mut m, &src, 5.0);
    assert_eq!(m.value(), 5.0);
    assert_eq!(m.default_string(), "five");
}

// ------------------------------------------------------------ value pipeline

#[test]
fn invert_and_relative_value() {
    let (s, mut m, src) = feed("MinValue=0\nMaxValue=100\nInvertMeasure=1");
    step(&s, &mut m, &src, 30.0);
    assert_eq!(m.raw_value(), 30.0);
    assert_eq!(m.value(), 70.0);
    assert_eq!(m.relative_value(), 0.7);
}

#[test]
fn relative_value_of_empty_range_is_one() {
    let (s, mut m, src) = feed("MinValue=5\nMaxValue=5");
    step(&s, &mut m, &src, 3.0);
    assert_eq!(m.relative_value(), 1.0);
}

#[test]
fn average_size_smooths_with_ring_buffer() {
    let (s, mut m, src) = feed("AverageSize=3");
    let got: Vec<f64> = [30.0, 60.0, 90.0, 0.0].iter().map(|v| {
        step(&s, &mut m, &src, *v);
        m.value()
    }).collect();
    // buffer starts filled with the first value
    assert_eq!(got, [30.0, 40.0, 60.0, 50.0]);
}

#[test]
fn update_divider_counts_ticks() {
    let (s, mut m, src) = feed("UpdateDivider=3");
    let ran: Vec<bool> = (0..7).map(|i| step(&s, &mut m, &src, i as f64)).collect();
    assert_eq!(ran, [true, false, false, true, false, false, true]);
}

#[test]
fn negative_divider_only_updates_when_forced() {
    let (s, mut m, src) = feed("UpdateDivider=-1");
    assert!(!step(&s, &mut m, &src, 1.0));
    src.set(7.0);
    let c = src.clone();
    let lk = move |n: &str| n.eq_ignore_ascii_case("Src").then(|| c.get());
    let mut log = Vec::new();
    let dir = s.env.skins_dir.join("T");
    let mut ctx = MeasureCtx { env: &s.env, skin_dir: &dir, update_ms: 1000, log: &mut log, lookup: &lk, counter: 1, host: None };
    assert!(m.force_update(&mut ctx, None));
    assert_eq!(m.value(), 7.0);
}

#[test]
fn disabled_and_paused() {
    let (s, mut m, src) = feed("");
    step(&s, &mut m, &src, 5.0);
    m.set_paused(true);
    assert!(!step(&s, &mut m, &src, 9.0));
    assert_eq!(m.value(), 5.0);
    m.set_paused(false);
    m.set_disabled(true);
    assert!(!step(&s, &mut m, &src, 9.0));
    assert_eq!(m.value(), 0.0);
    m.set_disabled(false);
    assert!(step(&s, &mut m, &src, 9.0));
    assert_eq!(m.value(), 9.0);

    let s = skin("[M]\nMeasure=Calc\nFormula=4\nDisabled=1\n[N]\nMeasure=Calc\nFormula=4\nPaused=1\n");
    assert!(s.measure("M").is_disabled());
    assert!(s.measure("N").is_paused());
}

#[test]
fn groups_are_split_and_lowercased() {
    let s = skin("[M]\nMeasure=Calc\nGroup=Net|Cpu\n");
    let m = s.measure("M");
    assert!(m.in_group("NET") && m.in_group("cpu") && !m.in_group("x"));
}

#[test]
fn wants_reread_with_dynamic_variables() {
    let s = skin("[M]\nMeasure=Calc\nFormula=1\nDynamicVariables=1\nUpdateDivider=2\n[N]\nMeasure=Calc\nFormula=1\n");
    let mut m = s.measure("M");
    assert!(m.wants_reread()); // first update always runs
    s.tick(&mut m, 1, &none);
    assert!(!m.wants_reread());
    s.tick(&mut m, 2, &none);
    assert!(m.wants_reread());
    assert!(!s.measure("N").wants_reread());
}

// ------------------------------------------------------------ auto max

struct Fed(Rc<Cell<f64>>);
impl MeasureImpl for Fed {
    fn read_options(&mut self, _o: &OptionReader) {}
    fn update(&mut self, _c: &mut MeasureCtx) -> f64 {
        self.0.get()
    }
    fn default_max(&self) -> f64 {
        0.0
    }
    fn auto_max(&self) -> bool {
        true
    }
}

#[test]
fn auto_max_uses_median_of_three() {
    let s = skin("[M]\nMeasure=Plugin\n");
    let src = Rc::new(Cell::new(0.0));
    let mut m = Measure::from_impl("M", "Plugin", Box::new(Fed(src.clone())), true, &s.opt("M"), vec![]);
    assert_eq!(m.max_value(), 1.0);
    // a single spike never moves the max (window starts filled with zeros)
    step_raw(&s, &mut m, &src, 500.0);
    assert_eq!(m.max_value(), 1.0);
    step_raw(&s, &mut m, &src, 10.0);
    assert_eq!(m.max_value(), 10.0); // median(500, 10, 0) = 10
    step_raw(&s, &mut m, &src, 20.0);
    assert_eq!(m.max_value(), 20.0); // median(500, 10, 20)
    step_raw(&s, &mut m, &src, 5.0);
    assert_eq!(m.max_value(), 20.0); // median(10, 20, 5) = 10
}

#[test]
fn explicit_max_disables_auto_max() {
    let s = skin("[M]\nMeasure=Plugin\nMaxValue=100\n");
    let src = Rc::new(Cell::new(0.0));
    let mut m = Measure::from_impl("M", "Plugin", Box::new(Fed(src.clone())), true, &s.opt("M"), vec![]);
    for v in [900.0, 900.0, 900.0] {
        step_raw(&s, &mut m, &src, v);
    }
    assert_eq!(m.max_value(), 100.0);
}

fn step_raw(s: &Skin, m: &mut Measure, src: &Rc<Cell<f64>>, v: f64) {
    src.set(v);
    s.tick(m, 1, &none);
}

// ------------------------------------------------------------ actions

#[test]
fn if_actions_through_measure_use_raw_value_and_other_measures() {
    let (s, mut m, src) = feed("IfAboveValue=50\nIfAboveAction=[!Above]\nIfCondition=Other > 1\nIfTrueAction=[!T]\nIfFalseAction=[!F]");
    let other = Rc::new(Cell::new(0.0));
    let o = other.clone();
    let c = src.clone();
    let lk = move |n: &str| match n.to_ascii_lowercase().as_str() {
        "src" => Some(c.get()),
        "other" => Some(o.get()),
        _ => None,
    };
    let mut fired = Vec::new();
    for (v, ov) in [(60.0, 0.0), (70.0, 5.0), (40.0, 5.0), (60.0, 0.0)] {
        src.set(v);
        other.set(ov);
        s.tick(&mut m, 1, &lk);
        fired.push(m.take_actions().join(""));
    }
    assert_eq!(fired, ["[!Above][!F]", "[!T]", "", "[!Above][!F]"]);
}

#[test]
fn if_condition_can_reference_the_measure_itself() {
    let (s, mut m, src) = feed("InvertMeasure=1\nMaxValue=100\nIfCondition=M = 70\nIfTrueAction=[!Self]");
    step(&s, &mut m, &src, 30.0); // GetValue (inverted) = 70
    assert_eq!(m.take_actions(), ["[!Self]"]);
}

#[test]
fn on_update_and_on_change_actions() {
    let (s, mut m, src) = feed("OnUpdateAction=[!U]\nOnChangeAction=[!C]");
    let mut got = Vec::new();
    for v in [1.0, 1.0, 2.0, 2.0, 3.0] {
        step(&s, &mut m, &src, v);
        got.push(m.take_actions().join(""));
    }
    // OnChange never fires on the first update, then whenever the value differs
    assert_eq!(got, ["[!U]", "[!U]", "[!U][!C]", "[!U]", "[!U][!C]"]);
}

#[test]
fn on_change_sees_string_changes() {
    let mut s = skin("[S]\nMeasure=String\nString=a\nDynamicVariables=1\nOnChangeAction=[!C]\n");
    let mut m = s.measure("S");
    s.tick(&mut m, 1, &none);
    assert!(m.take_actions().is_empty());
    s.tick(&mut m, 2, &none);
    assert!(m.take_actions().is_empty());
    s.p.set_option("S", "String", "b");
    m.read_options(&s.opt("S"));
    s.tick(&mut m, 3, &none);
    assert_eq!(m.take_actions(), ["[!C]"]);
}

#[test]
fn actions_not_run_when_disabled_or_paused() {
    let (s, mut m, src) = feed("OnUpdateAction=[!U]");
    m.set_paused(true);
    step(&s, &mut m, &src, 1.0);
    assert!(m.take_actions().is_empty());
    m.set_paused(false);
    m.set_disabled(true);
    step(&s, &mut m, &src, 1.0);
    assert!(m.take_actions().is_empty());
}

#[test]
fn if_match_sees_substituted_string_and_numbers_are_empty() {
    let s = skin("[S]\nMeasure=String\nString=Tue\nSubstitute=\"Tue\":\"Di\"\nIfMatch=^Di$\nIfMatchAction=[!Y]\nIfNotMatchAction=[!N]\n");
    let mut m = s.measure("S");
    s.tick(&mut m, 1, &none);
    assert_eq!(m.take_actions(), ["[!Y]"]);
    let (s, mut m, src) = feed("IfMatch=^$\nIfMatchAction=[!E]");
    step(&s, &mut m, &src, 37.0);
    assert_eq!(m.take_actions(), ["[!E]"]);
}

// ------------------------------------------------------------ factory / end to end

#[test]
fn all_measure_types_can_be_built_and_updated() {
    let s = skin(
        "[Variables]\n[a]\nMeasure=CPU\n[b]\nMeasure=PhysicalMemory\n[c]\nMeasure=SwapMemory\n[d]\nMeasure=Memory\n\
         [e]\nMeasure=FreeDiskSpace\nDrive=C:\n[f]\nMeasure=Uptime\n[g]\nMeasure=Time\n[h]\nMeasure=Calc\nFormula=1+1\n\
         [i]\nMeasure=String\nString=x\n[j]\nMeasure=NetIn\n[k]\nMeasure=NetOut\n[l]\nMeasure=NetTotal\n[m]\nMeasure=Registry\n\
         [n]\nMeasure=Loop\n[o]\nMeasure=Plugin\nPlugin=Foo.dll\n[p]\nMeasure=Script\n[q]\nMeasure=WebParser\n",
    );
    for sec in "abcdefghijklmnopq".chars() {
        let sec = sec.to_string();
        let mut m = s.measure(&sec);
        assert!(m.known_type, "{sec}");
        s.tick(&mut m, 1, &none);
        s.tick(&mut m, 2, &none);
        assert!(m.value().is_finite(), "{sec}");
        let _ = m.default_string();
        let _ = m.take_log();
    }
    let mut calc = s.measure("h");
    s.tick(&mut calc, 1, &none);
    assert_eq!(calc.value(), 2.0);
    let mut cpu = s.measure("a");
    s.tick(&mut cpu, 1, &none);
    assert_eq!(cpu.max_value(), 100.0);
}
