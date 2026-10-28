//! Units and constants, written as text the way MathQuill writes a `$`
//! text block: `5\text{m/s}`.

use graph_expr::{analyze, RowAnalysis, RowInput, Value};

fn rows(latex: &[&str]) -> Vec<RowAnalysis> {
    let input: Vec<RowInput> = latex
        .iter()
        .enumerate()
        .map(|(i, l)| RowInput {
            id: i as u64,
            latex: l.to_string(),
            ..Default::default()
        })
        .collect();
    analyze(&input)
}

/// The last row's value and the unit it is shown in.
fn shown(latex: &[&str]) -> (f64, Option<String>) {
    let last = rows(latex).pop().unwrap();
    match last.value {
        Some(Value::Number(v)) => (v, last.unit),
        _ => panic!("no value: {last:?}"),
    }
}

fn error(latex: &[&str]) -> String {
    let last = rows(latex).pop().unwrap();
    last.error
        .clone()
        .unwrap_or_else(|| panic!("no error: {last:?}"))
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

#[test]
fn converts_with_in() {
    let (v, unit) = shown(&["5\\text{m/s}\\cdot 3\\text{h}\\text{in km}"]);
    assert!(close(v, 54.0), "{v}");
    assert_eq!(unit.as_deref(), Some("\\text{km}"));
    let (v, _) = shown(&["60\\text{mph}\\text{ in km/h}"]);
    assert!(close(v, 96.56064), "{v}");
    let (v, _) = shown(&["1\\text{mi}\\text{ to ft}"]);
    assert!(close(v, 5280.0), "{v}");
}

#[test]
fn shows_answers_in_a_unit_that_was_written() {
    // Length, from the m in m/s.
    let (v, unit) = shown(&["5\\text{m/s}\\cdot 3\\text{h}"]);
    assert!(close(v, 54000.0));
    assert_eq!(unit.as_deref(), Some("\\text{m}"));
    let (v, unit) = shown(&["2\\text{km}+300\\text{m}"]);
    assert!(close(v, 2.3), "{v}");
    assert_eq!(unit.as_deref(), Some("\\text{km}"));
    // No unit written has the answer's dimension, so it is named in SI.
    let (v, unit) = shown(&["2\\text{kg}\\cdot 3\\text{m/s^2}"]);
    assert!(close(v, 6.0));
    assert_eq!(unit.as_deref(), Some("\\text{N}"));
}

#[test]
fn units_follow_variables_and_functions() {
    let (v, unit) = shown(&["v=5\\text{m/s}", "t=2\\text{min}", "v t"]);
    assert!(close(v, 600.0), "{v}");
    assert_eq!(unit.as_deref(), Some("\\text{m}"));
    let all = rows(&["d=3\\text{km}"]);
    assert_eq!(all[0].value, Some(Value::Number(3.0)));
    assert_eq!(all[0].slider, None);
    let (v, unit) = shown(&[
        "f\\left(t\\right)=9.8\\text{m/s^2}\\cdot t",
        "f\\left(2\\text{s}\\right)",
    ]);
    assert!(close(v, 19.6));
    assert_eq!(unit.as_deref(), Some("\\text{m}/\\text{s}"));
}

#[test]
fn constants() {
    let (v, unit) = shown(&["\\text{c}\\text{ in km/s}"]);
    assert!(close(v, 299_792.458));
    assert_eq!(unit.as_deref(), Some("\\text{km}/\\text{s}"));
    // E = mc² for a gram, in joules: a constant is not used as the unit.
    let (v, unit) = shown(&["1\\text{g}\\cdot\\text{c}^{2}"]);
    assert!(close(v, 1e-3 * 299_792_458f64.powi(2)));
    assert_eq!(unit.as_deref(), Some("\\text{J}"));
    let (v, _) = shown(&[
        "\\frac{\\text{G}\\cdot 5.972\\times10^{24}\\text{kg}}{\\left(6371\\text{km}\\right)^{2}}",
    ]);
    assert!((v - 9.82).abs() < 0.01, "{v}");
}

#[test]
fn angles_have_no_dimension() {
    let all = rows(&["\\sin\\left(30\\text{deg}\\right)"]);
    assert!(close(all[0].number().unwrap(), 0.5));
    assert_eq!(all[0].unit, None);
    let (v, _) = shown(&["180\\text{deg}\\text{ in rad}"]);
    assert!(close(v, std::f64::consts::PI));
}

#[test]
fn mismatched_units_are_errors() {
    assert!(error(&["2\\text{m}+3\\text{s}"]).contains("Can't add"));
    assert!(error(&["5\\text{kg}\\text{ in m}"]).contains("Can't convert"));
    assert!(error(&["\\sin\\left(2\\text{m}\\right)"]).contains("without units"));
    assert!(error(&["5\\text{furlongs}"]).contains("not a unit"));
}

#[test]
fn rows_without_units_are_unchanged() {
    let all = rows(&["a=2", "a+3", "y=x^{2}"]);
    assert_eq!(all[1].unit, None);
    assert_eq!(all[0].slider, Some(2.0));
    assert!(all.iter().all(|r| r.error.is_none()));
}

#[test]
fn units_scale_graphs() {
    let all = rows(&["y=x\\text{km}"]);
    assert!(all[0].error.is_none());
    assert!(close(all[0].curve().unwrap().program.eval(2.0), 2000.0));
}

#[test]
fn reads_mathquill_escapes() {
    let (v, unit) = shown(&["3\\text{m\\^{}2}"]);
    assert!(close(v, 3.0));
    assert_eq!(unit.as_deref(), Some("\\text{m}^{2}"));
}
