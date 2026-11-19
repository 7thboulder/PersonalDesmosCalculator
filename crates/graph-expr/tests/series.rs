//! Series as the reference panel's Series section inserts them: each sum
//! evaluates to its closed form, and functions of x match at a sample point.

use graph_expr::{analyze, RowInput, Value};

fn value(latex: &[&str]) -> f64 {
    let rows: Vec<RowInput> = latex
        .iter()
        .enumerate()
        .map(|(i, l)| RowInput {
            id: i as u64,
            latex: l.to_string(),
            ..Default::default()
        })
        .collect();
    let last = analyze(&rows).pop().unwrap();
    match last.value {
        Some(Value::Number(v)) => v,
        _ => panic!("no value for {latex:?}: {last:?}"),
    }
}

fn same(a: &[&str], b: &[&str]) {
    let (x, y) = (value(a), value(b));
    assert!(
        (x - y).abs() <= 1e-6 * (1.0 + y.abs()),
        "{a:?} = {x}, {b:?} = {y}"
    );
}

/// f(x) = series and g(x) = closed form agree at x = 0.3.
fn same_at(series: &str, closed: &str) {
    same(
        &[
            &format!("f\\left(x\\right)={series}"),
            "f\\left(0.3\\right)",
        ],
        &[
            &format!("g\\left(x\\right)={closed}"),
            "g\\left(0.3\\right)",
        ],
    );
}

#[test]
fn binomial_coefficients() {
    assert_eq!(value(&["\\binom{5}{2}"]), 10.0);
    assert_eq!(value(&["\\binom{6}{0}"]), 1.0);
    assert_eq!(value(&["n=7", "\\binom{n}{3}+1"]), 36.0);
    // A real top, as in the binomial series: (1/2)(−1/2)(−3/2)/3! = 1/16.
    same(&["\\binom{0.5}{3}"], &["\\frac{1}{16}"]);
    same(&["\\binom{-1}{4}"], &["1"]);
}

#[test]
fn numeric_sums() {
    let pairs: &[(&str, &str)] = &[
        ("\\sum_{n=1}^{\\infty}\\frac{1}{n^2}", "\\frac{\\pi^2}{6}"),
        (
            "\\sum_{n=1}^{\\infty}\\frac{\\left(-1\\right)^{n+1}}{n}",
            "\\ln \\left(2\\right)",
        ),
        (
            "\\sum_{n=0}^{\\infty}\\frac{\\left(-1\\right)^n}{2n+1}",
            "\\frac{\\pi}{4}",
        ),
        (
            "\\sum_{n=1}^{\\infty}\\frac{1}{n\\left(n+2\\right)}",
            "\\frac{3}{4}",
        ),
        ("\\sum_{n=0}^{\\infty}\\frac{1}{n!}", "e"),
        ("\\sum_{n=1}^{\\infty}\\frac{n}{2^n}", "2"),
        (
            "\\sum_{n=1}^{\\infty}\\frac{1}{\\left(2n-1\\right)^2}",
            "\\frac{\\pi^2}{8}",
        ),
    ];
    for (a, b) in pairs {
        same(&[a], &[b]);
    }
    same(
        &["n=9", "\\sum_{k=1}^{n}k^4"],
        &[
            "n=9",
            "\\frac{n\\left(n+1\\right)\\left(2n+1\\right)\\left(3n^2+3n-1\\right)}{30}",
        ],
    );
    same(&["n=9", "\\sum_{k=1}^{n}\\left(2k-1\\right)"], &["81"]);
    same(
        &["a=2", "q=0.4", "\\sum_{n=0}^{\\infty}aq^n"],
        &["\\frac{2}{0.6}"],
    );
}

#[test]
fn maclaurin_series() {
    same_at("\\sum_{n=0}^{\\infty}\\frac{x^n}{n!}", "e^x");
    same_at(
        "\\sum_{n=0}^{\\infty}\\frac{\\left(-1\\right)^nx^{2n+1}}{\\left(2n+1\\right)!}",
        "\\sin x",
    );
    same_at(
        "\\sum_{n=1}^{\\infty}\\frac{\\left(-1\\right)^{n+1}x^n}{n}",
        "\\ln \\left(1+x\\right)",
    );
    same_at(
        "\\sum_{n=0}^{\\infty}\\frac{\\left(-1\\right)^nx^{2n+1}}{2n+1}",
        "\\arctan x",
    );
    same_at(
        "\\sum_{n=0}^{\\infty}\\frac{\\left(2n\\right)!}{4^n\\left(n!\\right)^2\\left(2n+1\\right)}x^{2n+1}",
        "\\arcsin x",
    );
    same_at("\\sum_{n=0}^{\\infty}\\binom{0.5}{n}x^n", "\\sqrt{1+x}");
}

#[test]
fn fourier_series() {
    // The square wave converges slowly (a jump), so only to a few places.
    let square = value(&[
        "f\\left(x\\right)=\\frac{4}{\\pi}\\sum_{n=1}^{\\infty}\\frac{\\sin \\left(\\left(2n-1\\right)x\\right)}{2n-1}",
        "f\\left(0.3\\right)",
    ]);
    assert!((square - 1.0).abs() < 1e-4, "square wave at 0.3 = {square}");
    same_at(
        "2\\sum_{n=1}^{\\infty}\\frac{\\left(-1\\right)^{n+1}\\sin \\left(nx\\right)}{n}",
        "x",
    );
    same_at(
        "\\frac{\\pi^2}{3}+4\\sum_{n=1}^{\\infty}\\frac{\\left(-1\\right)^n\\cos \\left(nx\\right)}{n^2}",
        "x^2",
    );
}
