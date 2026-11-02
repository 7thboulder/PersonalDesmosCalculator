//! Subscripts that hold expressions, like H_{2n-1}, typed the way
//! MathQuill writes them.

use graph_expr::{analyze, RowAnalysis, RowInput, Value};

fn input(latex: &[&str]) -> Vec<RowInput> {
    latex
        .iter()
        .enumerate()
        .map(|(i, l)| RowInput {
            id: i as u64,
            latex: l.to_string(),
            ..Default::default()
        })
        .collect()
}

fn rows(latex: &[&str]) -> Vec<RowAnalysis> {
    analyze(&input(latex))
}

fn value(latex: &[&str]) -> f64 {
    let last = rows(latex).pop().unwrap();
    match last.value {
        Some(Value::Number(v)) => v,
        _ => panic!("no value: {last:?}"),
    }
}

fn harmonic(n: u32) -> f64 {
    (1..=n).map(|k| 1.0 / k as f64).sum()
}

macro_rules! check {
    ($rows:expr, $want:expr) => {{
        let v = value(&$rows);
        let want: f64 = $want;
        assert!(
            (v - want).abs() <= 1e-9 * (1.0 + want.abs()),
            "{:?} = {v}, want {want}",
            $rows
        );
    }};
}

const H: &str = "H_n=\\sum_{k=1}^{n}\\frac{1}{k}";

#[test]
fn expression_subscripts() {
    check!([H, "n=3", "H_{2n-1}"], harmonic(5));
    // Typed with brackets, as H_(2n-1).
    check!([H, "n=3", "H_{\\left(2n-1\\right)}"], harmonic(5));
    check!([H, "m=4", "H_{m+1}+1"], harmonic(5) + 1.0);
    check!([H, "H_{\\frac{6}{2}}"], harmonic(3));
}

#[test]
fn plain_subscripts_use_the_definition() {
    check!([H, "H_4"], harmonic(4));
    check!([H, "H_{10}"], harmonic(10));
    check!([H, "m=2", "H_m"], harmonic(2));
    check!([H, "m=2", "H_{3m}"], harmonic(6));
    check!(
        [H, "\\sum_{j=1}^{3}H_j"],
        harmonic(1) + harmonic(2) + harmonic(3)
    );
    // A name a row sets is still that row.
    check!([H, "H_4=7", "H_4"], 7.0);
}

#[test]
fn a_row_that_uses_itself_is_refused() {
    // Recursion isn't supported yet; it must not loop forever.
    let out = rows(&["a_n=2a_{n-1}", "a_{10}"]);
    assert!(
        out[0].error.as_deref().unwrap_or("").contains("itself"),
        "{:?}",
        out[0]
    );
}

#[test]
fn graphs_in_x() {
    // A curve, sampled at x = 2.
    check!(
        [H, "f\\left(x\\right)=H_{x+1}", "f\\left(2\\right)"],
        harmonic(3)
    );
}

#[test]
fn bessel_orders_can_be_expressions() {
    let direct = value(&["J_2\\left(1.5\\right)"]);
    check!(["n=1", "J_{n+1}\\left(1.5\\right)"], direct);
    check!(["n=1", "J_{\\left(n+1\\right)}\\left(1.5\\right)"], direct);
}

#[test]
fn missing_definition_says_what_to_write() {
    let last = rows(&["n=2", "G_{2n-1}"]).pop().unwrap();
    let error = last.error.expect("an error");
    assert!(error.contains("G_n"), "{error}");
}

#[test]
fn solve_expands_subscripts() {
    let others = vec!["a_n=n^2+1".to_string()];
    let report = graph_expr::symbolic::run("a_{2x}", &others, None);
    let result = report.result.clone().unwrap_or_default();
    assert!(
        result.contains("4x^{2}+1") || result.contains("4x^2+1"),
        "{report:?}"
    );
}

#[test]
fn giac_gets_the_subscript_function() {
    let others = vec!["H_n=n^2".to_string()];
    let plan = graph_expr::cas::plan("H_{2x}=4", &others, None).unwrap();
    assert!(
        plan.setup.iter().any(|s| s.starts_with("QQHQU(QQn):=")),
        "{:?}",
        plan.setup
    );
    assert!(plan.command.contains("QQHQU((2*QQx))"), "{}", plan.command);
    let plan = graph_expr::cas::plan("H_3+x=0", &others, None).unwrap();
    assert!(
        plan.command.contains("QQHQU(3)") || plan.command.contains("QQHQU((3"),
        "{}",
        plan.command
    );
}
