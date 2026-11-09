//! Functions that call themselves, as Desmos allows:
//! f(n) = {n = 0: 1, n f(n-1)}.

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

fn last(latex: &[&str]) -> RowAnalysis {
    analyze(&input(latex)).pop().unwrap()
}

fn value(latex: &[&str]) -> f64 {
    let r = last(latex);
    match r.value {
        Some(Value::Number(v)) => v,
        _ => panic!("no value: {r:?}"),
    }
}

const FACT: &str = "f\\left(n\\right)=\\left\\{n=0:1,nf\\left(n-1\\right)\\right\\}";
const FIB: &str =
    "F\\left(n\\right)=\\left\\{n\\le1:n,F\\left(n-1\\right)+F\\left(n-2\\right)\\right\\}";

#[test]
fn factorial() {
    assert_eq!(value(&[FACT, "f\\left(5\\right)"]), 120.0);
    assert_eq!(value(&[FACT, "f\\left(0\\right)"]), 1.0);
    assert_eq!(value(&[FACT, "f\\left(3\\right)+f\\left(4\\right)"]), 30.0);
}

#[test]
fn fibonacci_is_fast() {
    // 2^80 calls without remembering results.
    assert_eq!(value(&[FIB, "F\\left(80\\right)"]), 23416728348467685.0);
}

#[test]
fn deep_calls() {
    let g = "g\\left(n\\right)=\\left\\{n\\le0:0,g\\left(n-1\\right)+1\\right\\}";
    assert_eq!(value(&[g, "g\\left(5000\\right)"]), 5000.0);
}

#[test]
fn uses_sliders_and_other_functions() {
    let rows = [
        "a=2",
        "h\\left(x\\right)=x+a",
        "p\\left(n\\right)=\\left\\{n=0:1,h\\left(1\\right)p\\left(n-1\\right)\\right\\}",
        "p\\left(3\\right)",
    ];
    assert_eq!(value(&rows), 27.0);
}

#[test]
fn mutual_recursion() {
    let rows = [
        "e_{v}\\left(n\\right)=\\left\\{n=0:1,o_{d}\\left(n-1\\right)\\right\\}",
        "o_{d}\\left(n\\right)=\\left\\{n=0:0,e_{v}\\left(n-1\\right)\\right\\}",
        "e_{v}\\left(10\\right)+o_{d}\\left(7\\right)",
    ];
    assert_eq!(value(&rows), 2.0);
}

#[test]
fn lists_broadcast() {
    let r = last(&[FACT, "f\\left(\\left[1,2,3\\right]\\right)"]);
    match r.value {
        Some(Value::Numbers(v)) => assert_eq!(v, vec![1.0, 2.0, 6.0]),
        _ => panic!("{r:?}"),
    }
}

#[test]
fn no_base_case_is_an_error() {
    let r = last(&[
        "f\\left(n\\right)=f\\left(n-1\\right)+1",
        "f\\left(3\\right)",
    ]);
    let e = r.error.expect("an error");
    assert!(e.contains("base case"), "{e}");
}

#[test]
fn graphs_against_x() {
    // y = f(floor(x)) draws as a curve worked out when it runs.
    let r = last(&[
        FACT,
        "y=f\\left(\\operatorname{floor}\\left(x\\right)\\right)",
    ]);
    assert!(r.error.is_none(), "{r:?}");
    let curve = r.curves.first().expect("a curve");
    assert_eq!(curve.program.eval(4.5), 24.0);
    assert!(curve.program.eval(-1.0).is_nan());
}

#[test]
fn derivative_of_a_recursive_function() {
    let g = "g\\left(x\\right)=\\left\\{x\\le0:x^{2},g\\left(x-1\\right)+1\\right\\}";
    let r = last(&[g, "y=\\frac{d}{dx}g\\left(x\\right)"]);
    assert!(r.error.is_none(), "{r:?}");
    // g(1.5) = g(-0.5) + 2 = 0.25 + 2, so the slope is 2(-0.5).
    let slope = r.curves[0].program.eval(1.5);
    assert!((slope + 1.0).abs() < 1e-4, "{slope}");
}

#[test]
fn solve_reads_rows_with_recursion() {
    // Solve can't expand a function that calls itself, but must not hang.
    let r = graph_expr::symbolic::run("f\\left(3\\right)", &[FACT.to_string()], None);
    eprintln!("{r:?}");
    let r = graph_expr::symbolic::run("x^{2}=4", &[FACT.to_string()], None);
    assert!(r.error.is_none(), "{r:?}");
}

#[test]
fn undefined_values_dont_follow_the_calls() {
    let r = last(&[FIB, "\\frac{F\\left(30\\right)}{0}"]);
    eprintln!("{r:?}");
}
