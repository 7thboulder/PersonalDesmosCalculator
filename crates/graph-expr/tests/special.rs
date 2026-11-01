//! Special functions typed the way MathQuill writes them.

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

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * (1.0 + b.abs())
}

macro_rules! check {
    ($rows:expr, $want:expr) => {{
        let v = value(&$rows);
        assert!(close(v, $want), "{:?} = {v}, want {}", $rows, $want);
    }};
}

#[test]
fn gamma_and_friends() {
    check!(["\\Gamma\\left(5\\right)"], 24.0);
    check!(["\\Gamma\\left(\\frac{1}{2}\\right)"], std::f64::consts::PI.sqrt());
    check!(["\\psi\\left(1\\right)"], -0.577_215_664_901_532_9);
    check!(["B\\left(2,3\\right)"], 1.0 / 12.0);
    // In terms of other functions and inside user functions.
    check!(["f\\left(x\\right)=\\Gamma\\left(x+1\\right)", "f\\left(4\\right)"], 24.0);
}

#[test]
fn a_defined_name_is_not_a_special_function() {
    // Γ = 3 makes Γ(2) a product, as in Desmos.
    check!(["\\Gamma=3", "\\Gamma\\left(2\\right)"], 6.0);
    check!(["W\\left(x\\right)=x^{2}", "W\\left(3\\right)"], 9.0);
    // A parameter named K is a number inside the function.
    check!(["g\\left(K\\right)=K\\left(2\\right)", "g\\left(5\\right)"], 10.0);
}

#[test]
fn named_functions() {
    check!(["\\operatorname{erf}\\left(1\\right)"], 0.842_700_792_949_714_9);
    check!(["\\operatorname{erfc}\\left(2\\right)"], 0.004_677_734_981_047_266);
    check!(["\\operatorname{erfi}\\left(1\\right)"], 1.650_425_758_797_543);
    check!(["\\operatorname{Si}\\left(1\\right)"], 0.946_083_070_367_183);
    check!(["\\operatorname{Ci}\\left(1\\right)"], 0.337_403_922_900_968_1);
    check!(["\\operatorname{Ei}\\left(1\\right)"], 1.895_117_816_355_937);
    check!(["\\operatorname{li}\\left(10\\right)"], 6.165_599_504_787_298);
    check!(["\\operatorname{Ai}\\left(1\\right)"], 0.135_292_416_312_881_4);
    check!(["\\operatorname{Bi}\\left(1\\right)"], 1.207_423_594_952_871);
    check!(["\\operatorname{Li}_{2}\\left(0.5\\right)"], 0.582_240_526_465_012_5);
}

#[test]
fn letters_with_orders() {
    check!(["\\zeta\\left(2\\right)"], std::f64::consts::PI.powi(2) / 6.0);
    check!(["J_{0}\\left(1\\right)"], 0.765_197_686_557_966_6);
    check!(["n=2", "J_{n}\\left(10\\right)"], 0.254_630_313_685_120_6);
    check!(["Y_{0}\\left(1\\right)"], 0.088_256_964_215_676_96);
    check!(["I_{1}\\left(20\\right)"], 4.245_497_338_512_777e7);
    check!(["K_{0}\\left(1\\right)"], 0.421_024_438_240_708_3);
    check!(["W\\left(1\\right)"], 0.567_143_290_409_783_8);
    check!(["K\\left(0.5\\right)"], 1.685_750_354_812_596);
    check!(["E\\left(0.5\\right)"], 1.467_462_209_339_427);
}

#[test]
fn special_functions_graph() {
    let r = rows(&["y=\\Gamma\\left(x\\right)"]);
    assert!(r[0].error.is_none(), "{:?}", r[0].error);
    assert_eq!(r[0].curves.len(), 1);
    let r = rows(&["y=J_{0}\\left(x\\right)"]);
    assert!(r[0].error.is_none(), "{:?}", r[0].error);
    assert_eq!(r[0].curves.len(), 1);
}

#[test]
fn complex_gamma_and_zeta() {
    let r = rows(&["\\zeta\\left(2+i\\right)"]);
    match &r[0].value {
        Some(Value::Complex(re, im)) => {
            assert!(close(*re, 1.150_355_703_254_903), "{re}");
            assert!(close(*im, -0.437_530_865_919_607_9), "{im}");
        }
        other => panic!("{other:?} {:?}", r[0].error),
    }
}
