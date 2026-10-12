use snowmeter_core::formula::evaluate;

fn none(_: &str) -> Option<f64> {
    None
}
fn ev(s: &str) -> Result<f64, String> {
    evaluate(s, &none)
}
fn ok(s: &str, expect: f64) {
    match ev(s) {
        Ok(v) => assert!((v - expect).abs() < 1e-9, "{s:?} = {v}, expected {expect}"),
        Err(e) => panic!("{s:?} errored: {e}"),
    }
}
fn err(s: &str) {
    assert!(ev(s).is_err(), "{s:?} should be an error, got {:?}", ev(s));
}

#[test]
fn basics_and_brackets() {
    ok("5", 5.0);
    ok("      5      ", 5.0);
    ok("((((5))))", 5.0);
    ok("(5 + 5)", 10.0);
    ok("(-5 - 5)", -10.0);
    ok("-(-5-5)", 10.0);
    ok("-(-5+-5)", 10.0);
    ok("", 0.0);
}

#[test]
fn logic_and_comparison_share_priority() {
    ok("1 && 1", 1.0);
    ok("1 && 0", 0.0);
    ok("1 || 1", 1.0);
    ok("1 || -1", 1.0);
    ok("1 < 2 && 2 > 1", 0.0);
    ok("(1 < 2) && (2 > 1)", 1.0);
    ok("1=1", 1.0);
    ok("1<>2", 1.0);
    ok("2>=2", 1.0);
    ok("1<=0", 0.0);
}

#[test]
fn functions_and_unary_minus() {
    ok("sin(50)", 50f64.sin());
    ok("-sin((25 + 25) * 2)", -(100f64.sin()));
    ok("sin 50", 50f64.sin());
    ok("COS(0)", 1.0);
    ok("sqrt(16)", 4.0);
    ok("abs(-3)", 3.0);
    ok("log(100)", 2.0);
    ok("ln(e)", 1.0);
    ok("exp(0)", 1.0);
    ok("floor(1.7)", 1.0);
    ok("ceil(1.2)", 2.0);
    ok("sgn(-4)", -1.0);
    ok("deg(pi)", 180.0);
    ok("rad(180)", std::f64::consts::PI);
    ok("frac(-1.5)", -0.5);
    ok("atan2(1, 1)", std::f64::consts::FRAC_PI_4);
    ok("asin(1)", std::f64::consts::FRAC_PI_2);
    ok("acos(1)", 0.0);
    ok("tan(0)", 0.0);
    ok("atan(0)", 0.0);
    ok("neg(5)", -5.0);
}

#[test]
fn number_literals() {
    ok("01", 1.0);
    ok("011", 11.0);
    ok("0b11", 3.0);
    ok("0xA", 10.0);
    ok("0o12", 10.0);
    ok("1e3", 1000.0);
    ok("2.5E-2", 0.025);
    ok("0X1F", 31.0);
    err(".5");
}

#[test]
fn ternary() {
    ok("1 ? 2 : 3", 2.0);
    ok("0 ? 2 : 3", 3.0);
    ok("1 ? (2 + 3) : 3", 5.0);
    ok("1 ? 2 : 0 ? 4 : 5", 4.0);
    ok("1 ? 2 : 1 ? 4 : 5", 4.0);
    ok("1 ? 2 : (1 ? 4 : 5)", 2.0);
    ok("0 ? 1 : 2 > 1", 1.0);
    let deep = format!("{}5{}", "0?1:(".repeat(30), ")".repeat(30));
    ok(&deep, 5.0);
    err("1 ? 0 ? 4 : 5 : 3");
    err("0?1/0:2");
}

#[test]
fn rounding_trunc_minmax_clamp() {
    ok("trunc(1.5)", 1.0);
    ok("trunc(-1.5)", -1.0);
    ok("round(1.555)", 2.0);
    ok("round(1.555, 2)", 1.56);
    ok("round(155,-1)", 160.0);
    ok("round(-2.5)", -3.0);
    ok("min(1, -2)", -2.0);
    ok("max(3, -2)", 3.0);
    ok("clamp(6, -2, 3)", 3.0);
    ok("clamp(-6, -2, 3)", -2.0);
    err("min(1)");
    err("min(1,2,3)");
    err("clamp(1,2)");
}

#[test]
fn power_xor_and_bit_ops() {
    ok("2^3", 1.0);
    ok("2**3**2", 64.0);
    ok("-2**2", 4.0);
    ok("2*3&1", 2.0);
    ok("5.9 & 3", 1.0);
    ok("5 | 2", 7.0);
    ok("~0", -1.0);
    ok("1<<4", 16.0);
    ok("256>>4", 16.0);
}

#[test]
fn modulo_and_division() {
    ok("-7%3", -1.0);
    ok("7%3", 1.0);
    assert!(ev("5%0").unwrap().is_nan());
    ok("7/2", 3.5);
    assert_eq!(ev("1 / 0").unwrap_err(), "Division by 0");
}

#[test]
fn constants_beat_resolver() {
    ok("e", std::f64::consts::E);
    ok("pi", std::f64::consts::PI);
    let r = |n: &str| if n == "pi" { Some(40.0) } else { None };
    assert!((evaluate("pi", &r).unwrap() - std::f64::consts::PI).abs() < 1e-12);
}

#[test]
fn measure_lookup_through_closure() {
    let r = |n: &str| match n.to_ascii_lowercase().as_str() {
        "a" => Some(10.0),
        "bbb" => Some(20.0),
        "ccc_" => Some(30.0),
        "meter.x" => Some(7.0),
        _ => None,
    };
    assert_eq!(evaluate("a + 5", &r).unwrap(), 15.0);
    assert_eq!(evaluate("bbb", &r).unwrap(), 20.0);
    assert_eq!(evaluate("(ccc_)", &r).unwrap(), 30.0);
    assert_eq!(evaluate("A*2", &r).unwrap(), 20.0);
    assert_eq!(evaluate("meter.x*2", &r).unwrap(), 14.0);
    assert_eq!(evaluate("-a", &r).unwrap(), -10.0);
    assert_eq!(evaluate("max(a, bbb)", &r).unwrap(), 20.0);
    assert_eq!(evaluate("zzz", &r).unwrap_err(), "\"zzz\" is unknown");
}

#[test]
fn errors() {
    err("++1");
    err("((1)");
    err("1 &&");
    err("a");
    err("1==1");
    err("2*-x");
    err("2*-(3)");
    err("1 - -pi");
    err("2pi");
    err("1\r");
    err("1 , 2");
    assert_eq!(ev("((1)").unwrap_err(), "Unmatched brackets");
}

#[test]
fn unary_minus_special_cases() {
    ok("--1", 1.0);
    ok("2*-3", -6.0);
    ok("(5)-2", 3.0);
    ok("(-pi/2)", -std::f64::consts::FRAC_PI_2);
    ok("(-(5)-2)", -7.0);
    ok("-(3)", -3.0);
    ok("1-1", 0.0);
}

#[test]
fn precedence_and_associativity() {
    ok("2+3*4", 14.0);
    ok("10-4-3", 3.0);
    ok("100/10/5", 2.0);
    ok("(2+3)*4", 20.0);
    ok("8/4*2", 4.0);
}

#[test]
fn nested_and_chained_ternaries_from_real_skins() {
    ok("(2=2 ? (1<3 ? 1 : 0) : 2)", 1.0);
    ok("(2=2 ? (5<3 ? 1 : 0) : 7)", 0.0);
    ok("(3=2 ? (1<3 ? 1 : 0) : 7)", 7.0);
    ok("5<7 ? 2 : (5<12 ? 3 : 4)", 2.0);
    ok("9<7 ? 2 : (9<12 ? 3 : 4)", 3.0);
    ok("13<7 ? 2 : (13<12 ? 3 : 4)", 4.0);
    ok("(9<7 ? 2 : 9<12 ? 3 : 4)", 3.0);
}

#[test]
fn persona5_calendar_formulas() {
    ok("(2 = 2 ? (5 < 3 ? 1 : 0) : 2)", 0.0);
    ok("(8 < 10 ? 5 : (5*100))", 5.0);
    for (h, e) in [(0, 1.0), (5, 2.0), (8, 3.0), (12, 4.0), (15, 5.0), (20, 6.0), (23, 1.0)] {
        ok(&format!("{h} < 1 ? 1 : ({h}<7 ? 2 : ({h}<11 ? 3 : ({h}<14 ? 4 : ({h}<19 ? 5 : ({h}<21 ? 6 : 1)))))"), e);
    }
    ok("1 % 3 + (8 < 10 ? 3 : 0)", 4.0);
}
