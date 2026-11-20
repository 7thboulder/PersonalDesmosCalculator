//! Names with a hat over the letter, like k̂, typed as MathQuill writes
//! them: `\hat{k}`. A hatted letter is a name of its own, apart from the
//! plain letter.

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

fn value(latex: &[&str]) -> f64 {
    let last = rows(latex).pop().unwrap();
    match last.value {
        Some(Value::Number(v)) => v,
        _ => panic!("no value: {last:?}"),
    }
}

#[test]
fn hatted_letters_are_their_own_names() {
    assert_eq!(value(&["\\hat{k}=3", "k=5", "2\\hat{k}+k"]), 11.0);
    assert_eq!(value(&["\\hat{\\theta}=2", "\\hat{\\theta}^{2}"]), 4.0);
    assert_eq!(value(&["\\hat{x}_{1}=7", "\\hat{x}_{1}-1"]), 6.0);
    // \hat k without braces, as hand-typed LaTeX often has it.
    assert_eq!(value(&["\\hat k=4", "\\hat{k}"]), 4.0);
}

#[test]
fn hatted_functions_and_graphs() {
    assert_eq!(
        value(&["\\hat{f}\\left(x\\right)=x+1", "\\hat{f}\\left(2\\right)"]),
        3.0
    );
    let r = rows(&["y=\\hat{a}x", "\\hat{a}=2"]);
    assert!(r[0].error.is_none(), "{:?}", r[0].error);
}

#[test]
fn undefined_hat_names_the_hat() {
    let r = rows(&["\\hat{k}+1"]);
    let err = r[0].error.clone().unwrap_or_default();
    assert!(!err.is_empty());
    assert!(
        !err.contains('\u{302}') || err.contains("k\u{302}"),
        "{err}"
    );
}
