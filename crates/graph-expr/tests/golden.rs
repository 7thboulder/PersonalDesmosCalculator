//! Golden tests: expressions exactly as MathQuill writes them, with the
//! result Desmos gives. This is the Phase 1 gate for matching Desmos syntax.

use graph_expr::{analyze, Axis, RowAnalysis, RowInput, RowKind};

fn rows(latex: &[&str]) -> Vec<RowAnalysis> {
    let input: Vec<RowInput> = latex
        .iter()
        .enumerate()
        .map(|(i, l)| RowInput {
            id: i as u64,
            latex: l.to_string(),
        })
        .collect();
    analyze(&input)
}

fn one(latex: &str) -> RowAnalysis {
    rows(&[latex]).remove(0)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + b.abs()) || (a.is_nan() && b.is_nan())
}

/// The curve of the last row, evaluated at `x`.
fn curve_at(latex: &[&str], x: f64) -> f64 {
    let all = rows(latex);
    let last = all.last().unwrap();
    let curve = last
        .curve
        .as_ref()
        .unwrap_or_else(|| panic!("no curve: {last:?}"));
    curve.program.eval(x)
}

fn value(latex: &[&str]) -> f64 {
    let all = rows(latex);
    let last = all.last().unwrap();
    last.value.unwrap_or_else(|| panic!("no value: {last:?}"))
}

#[test]
fn constant_values() {
    let cases: &[(&str, f64)] = &[
        ("2+3\\cdot4", 14.0),
        ("2\\times3", 6.0),
        ("\\frac{1}{2}+\\frac{1}{4}", 0.75),
        ("2^{10}", 1024.0),
        ("-2^2", -4.0),
        ("\\left(-2\\right)^2", 4.0),
        ("\\sqrt{16}", 4.0),
        ("\\sqrt[3]{-27}", -3.0),
        ("\\left|-5\\right|", 5.0),
        ("5!", 120.0),
        ("\\sin\\left(\\frac{\\pi}{2}\\right)", 1.0),
        ("\\cos\\pi", -1.0),
        ("\\ln e", 1.0),
        ("\\log100", 2.0),
        ("\\log_2 8", 3.0),
        ("\\exp\\left(0\\right)", 1.0),
        ("\\operatorname{floor}\\left(2.7\\right)", 2.0),
        ("\\operatorname{ceil}\\left(2.1\\right)", 3.0),
        ("\\operatorname{round}\\left(2.5\\right)", 3.0),
        ("\\operatorname{mod}\\left(-1,3\\right)", 2.0),
        ("\\max\\left(1,5,3\\right)", 5.0),
        ("\\min\\left(4,-2\\right)", -2.0),
        ("\\gcd\\left(12,18\\right)", 6.0),
        ("\\operatorname{nCr}\\left(5,2\\right)", 10.0),
        ("\\operatorname{mean}\\left(1,2,3,4\\right)", 2.5),
        ("\\arctan\\left(1,1\\right)", std::f64::consts::FRAC_PI_4),
        ("\\sin^2\\left(1\\right)+\\cos^2\\left(1\\right)", 1.0),
        ("\\sin^{-1}\\left(1\\right)", std::f64::consts::FRAC_PI_2),
        ("2\\pi", std::f64::consts::TAU),
        ("\\tau", std::f64::consts::TAU),
        ("\\left(-8\\right)^{\\frac{1}{3}}", -2.0),
    ];
    for (latex, want) in cases {
        let got = value(&[latex]);
        assert!(close(got, *want), "{latex}: got {got}, want {want}");
    }
}

#[test]
fn explicit_curves() {
    assert!(close(curve_at(&["y=x^2"], 3.0), 9.0));
    assert!(
        close(curve_at(&["x^2"], -3.0), 9.0),
        "a bare expression in x is graphed"
    );
    assert!(close(curve_at(&["y=2x+1"], 2.0), 5.0));
    assert!(close(curve_at(&["y=\\sin x"], 1.0), 1f64.sin()));
    assert!(close(curve_at(&["y=\\sin2x"], 1.0), 2f64.sin()));
    assert!(close(curve_at(&["y=x\\sin x"], 2.0), 2.0 * 2f64.sin()));
    assert!(close(curve_at(&["y=\\frac{1}{x}"], 4.0), 0.25));
    assert!(close(curve_at(&["y=x^{\\frac{1}{3}}"], -8.0), -2.0));
    assert!(close(curve_at(&["y=\\left|x\\right|"], -2.0), 2.0));
    assert!(
        close(curve_at(&["2x=y"], 3.0), 6.0),
        "y on the right also works"
    );
    assert!(curve_at(&["y=\\sqrt{x}"], -1.0).is_nan());
}

#[test]
fn x_equals_curves() {
    let all = rows(&["x=y^2"]);
    let curve = all[0].curve.as_ref().unwrap();
    assert_eq!(curve.axis, Axis::Y);
    assert!(close(curve.program.eval(3.0), 9.0));

    let vertical = rows(&["x=3"]);
    assert_eq!(vertical[0].curve.as_ref().unwrap().axis, Axis::Y);
}

#[test]
fn variables_and_sliders() {
    let all = rows(&["a=2", "y=ax"]);
    assert_eq!(all[0].kind, RowKind::Variable("a".into()));
    assert_eq!(all[0].slider, Some(2.0));
    assert!(close(all[1].curve.as_ref().unwrap().program.eval(3.0), 6.0));

    // Definitions can come after their use, and chain.
    let all = rows(&["y=bx", "b=2a", "a=-1.5"]);
    assert!(close(
        all[0].curve.as_ref().unwrap().program.eval(2.0),
        -6.0
    ));
    assert_eq!(all[1].slider, None);
    assert!(close(all[1].value.unwrap(), -3.0), "b = 2a shows its value");
    assert_eq!(all[2].slider, Some(-1.5));

    // Subscripted names.
    assert!(close(curve_at(&["a_1=3", "y=a_1x"], 2.0), 6.0));
}

#[test]
fn undefined_names_offer_sliders() {
    let r = one("y=mx+b");
    assert_eq!(r.kind, RowKind::Curve);
    assert_eq!(r.missing, vec!["m".to_string(), "b".to_string()]);
    assert!(r.curve.is_none());
    assert_eq!(r.error.as_deref(), Some("'m', 'b' are not defined"));
}

#[test]
fn functions() {
    let all = rows(&[
        "f\\left(x\\right)=x^2+1",
        "y=f\\left(x-1\\right)",
        "f\\left(3\\right)",
    ]);
    assert_eq!(all[0].kind, RowKind::Function("f".into()));
    assert!(
        close(all[0].curve.as_ref().unwrap().program.eval(2.0), 5.0),
        "f(x)=... is graphed"
    );
    assert!(close(all[1].curve.as_ref().unwrap().program.eval(3.0), 5.0));
    assert!(close(all[2].value.unwrap(), 10.0));

    // Two parameters, and a function of t that is not graphed.
    let all = rows(&[
        "g\\left(a,b\\right)=a-b",
        "g\\left(5,2\\right)",
        "h\\left(t\\right)=2t",
    ]);
    assert!(close(all[1].value.unwrap(), 3.0));
    assert!(all[2].curve.is_none() && all[2].error.is_none());

    // a(x+1) is multiplication when a is a variable.
    assert!(close(curve_at(&["a=3", "y=a\\left(x+1\\right)"], 1.0), 6.0));
}

#[test]
fn errors_match_desmos_intent() {
    assert_eq!(
        rows(&["a=1", "a=2"])[0].error.as_deref(),
        Some("'a' is defined more than once")
    );
    assert_eq!(
        one("a=a+1").error.as_deref(),
        Some("'a' is defined in terms of itself")
    );
    let cyc = rows(&["a=b", "b=a"]);
    assert!(cyc[0]
        .error
        .as_deref()
        .unwrap()
        .contains("in terms of itself"));
    assert_eq!(
        one("\\pi=3").error.as_deref(),
        Some("'pi' is a built-in constant and can't be redefined")
    );
    assert_eq!(
        rows(&["f\\left(x\\right)=x", "f\\left(1,2\\right)"])[1]
            .error
            .as_deref(),
        Some("Function 'f' takes 1 argument")
    );
    assert_eq!(one("y=").error.as_deref(), Some("Incomplete expression"));
    assert_eq!(
        one("y^2").error.as_deref(),
        Some("Try writing this as x = …")
    );
}

#[test]
fn not_yet_supported_is_labelled() {
    assert_eq!(one("x^2+y^2=1").kind, RowKind::NotYet);
    assert_eq!(one("y>x").kind, RowKind::NotYet);
    assert_eq!(one("r=\\theta").kind, RowKind::NotYet);
    assert_eq!(one("\\left[1,2,3\\right]").kind, RowKind::Error);
}

#[test]
fn empty_rows() {
    assert_eq!(one("").kind, RowKind::Empty);
    assert_eq!(one("  ").kind, RowKind::Empty);
}
