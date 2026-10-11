use snowmeter_core::bangs::{parse_actions, split_args, Action};

fn bang(name: &str, args: &[&str]) -> Action {
    Action::Bang { name: name.into(), args: args.iter().map(|s| s.to_string()).collect() }
}
fn v(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn split_args_vectors() {
    assert_eq!(split_args("Meter Text \"Hello World\""), v(&["Meter", "Text", "Hello World"]));
    assert_eq!(split_args("M Text \"\"\"a \"b\" c\"\"\""), v(&["M", "Text", "a \"b\" c"]));
    assert_eq!(split_args("ab\"cd"), v(&["abcd"]));
    assert_eq!(split_args("a   b"), v(&["a", "b"]));
    assert_eq!(split_args("M Text \"\""), v(&["M", "Text", ""]));
    assert_eq!(split_args("\"unterminated"), v(&["unterminated"]));
    assert_eq!(split_args(""), Vec::<String>::new());
    assert_eq!(split_args("   "), Vec::<String>::new());
}

#[test]
fn split_args_triple_quote_then_more() {
    assert_eq!(split_args("\"\"\"a b\"\"\" c"), v(&["a b", "c"]));
}

#[test]
fn single_bang() {
    assert_eq!(parse_actions("!SetVariable A 1"), vec![bang("setvariable", &["A", "1"])]);
    assert_eq!(parse_actions("  !Refresh"), vec![bang("refresh", &[])]);
}

#[test]
fn rainmeter_prefix_stripped() {
    assert_eq!(parse_actions("!RainmeterRefresh"), vec![bang("refresh", &[])]);
    assert_eq!(parse_actions("!RainmeterSetVariable A 1"), vec![bang("setvariable", &["A", "1"])]);
}

#[test]
fn bracket_groups() {
    assert_eq!(
        parse_actions("[!SetVariable A 1][!Redraw]"),
        vec![bang("setvariable", &["A", "1"]), bang("redraw", &[])]
    );
    assert_eq!(parse_actions("[!A] junk [!B]"), vec![bang("a", &[]), bang("b", &[])]);
    assert_eq!(parse_actions("[!Log \"a]b\"]"), vec![bang("log", &["a"])]);
    assert_eq!(parse_actions("[!Log \"\"\"a]b\"\"\"]"), vec![bang("log", &["a]b"])]);
}

#[test]
fn nested_brackets_stay_in_group() {
    assert_eq!(
        parse_actions("[!SetOption M Text \"[#X]\"]"),
        vec![bang("setoption", &["M", "Text", "[#X]"])]
    );
}

#[test]
fn delay_is_a_bang_in_sequence() {
    assert_eq!(
        parse_actions("[!Delay 500][!B][!C]"),
        vec![bang("delay", &["500"]), bang("b", &[]), bang("c", &[])]
    );
}

#[test]
fn plain_commands() {
    assert_eq!(parse_actions("notepad.exe"), vec![Action::Command("notepad.exe".into())]);
    assert_eq!(parse_actions("  https://example.com"), vec![Action::Command("https://example.com".into())]);
    assert_eq!(parse_actions("[\"C:\\Program Files\\x.exe\"]"), vec![Action::Command("\"C:\\Program Files\\x.exe\"".into())]);
    // INI quote stripping leaves this form; stays a plain command (split happens at run time)
    assert_eq!(
        parse_actions("C:\\Program Files\\x.exe"),
        vec![Action::Command("C:\\Program Files\\x.exe".into())]
    );
}

#[test]
fn empty_input() {
    assert!(parse_actions("").is_empty());
    assert!(parse_actions("   ").is_empty());
}

#[test]
fn execute_legacy_prefix() {
    assert_eq!(parse_actions("!Execute [!A][!B]"), vec![bang("a", &[]), bang("b", &[])]);
}
