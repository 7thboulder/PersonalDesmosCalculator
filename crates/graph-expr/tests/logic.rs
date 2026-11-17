//! Proof symbols in rows: statements, restrictions to sets, set operations.

use graph_expr::{analyze, RowAnalysis, RowInput, Value};

fn run(latex: &[&str]) -> Vec<RowAnalysis> {
    let rows: Vec<RowInput> = latex
        .iter()
        .enumerate()
        .map(|(i, l)| RowInput {
            id: i as u64 + 1,
            latex: (*l).into(),
            ..Default::default()
        })
        .collect();
    analyze(&rows)
}

fn verdict(rows: &[&str]) -> String {
    let out = run(rows);
    let last = out.last().unwrap();
    assert_eq!(last.error, None, "{rows:?}");
    last.verdict
        .clone()
        .unwrap_or_else(|| panic!("no verdict for {rows:?}: {last:?}"))
}

#[test]
fn quantifiers_over_number_systems() {
    let cases = [
        (
            "\\forall n\\in\\mathbb{N}:\\frac{1}{n}>0",
            "\\text{true}\\quad\\left(\\text{checked for }1\\le n\\le 10000\\right)",
        ),
        ("\\forall n\\in\\mathbb{N}:n^2>3", "\\text{false at }n=1"),
        ("\\exists n\\in\\mathbb{N}:n^2=49", "\\text{true: }n=7"),
        (
            "\\exists x\\in\\mathbb{R}:x^2=2",
            "\\text{true: }x=1.41421356237",
        ),
        (
            "\\nexists n\\in\\mathbb{Z}:n^2=2",
            "\\text{true}\\quad\\left(\\text{checked for }\\left|n\\right|\\le 5000\\right)",
        ),
        ("\\exists !x\\in\\mathbb{R}:x^3=8", "\\text{true: }x=2"),
        (
            "\\exists !x\\in\\mathbb{R}:x^2=4",
            "\\text{false: }x=2\\text{ and }x=-2",
        ),
        (
            "\\forall x\\in\\mathbb{R}:x^2\\ge 0",
            "\\text{true}\\quad\\left(\\text{checked numerically}\\right)",
        ),
        (
            "\\forall p\\in\\mathbb{P}:p=2\\vee \\operatorname{mod}\\left(p,2\\right)=1",
            "\\text{true}\\quad\\left(\\text{checked for }p\\le 104729\\right)",
        ),
    ];
    for (latex, want) in cases {
        assert_eq!(verdict(&[latex]), want, "{latex}");
    }
}

#[test]
fn epsilon_delta() {
    // Continuity of 2x at 1: for every ε > 0 there is a δ > 0 that works.
    let v =
        verdict(&["\\forall \\epsilon >0\\exists \\delta >0:\\delta \\le \\frac{\\epsilon }{2}"]);
    assert!(v.starts_with("\\text{true}"), "{v}");
    // An Archimedean property: no ε below every 1/n fails at the first one.
    let v = verdict(&["\\forall \\epsilon >0,\\exists n\\in \\mathbb{N}:\\frac{1}{n}<\\epsilon "]);
    assert!(v.starts_with("\\text{true}"), "{v}");
}

#[test]
fn logic_and_membership() {
    let cases = [
        ("3\\in \\mathbb{N}", "\\text{true}"),
        ("-3\\in \\mathbb{N}", "\\text{false}"),
        ("0\\in \\mathbb{N}_0", "\\text{true}"),
        ("\\frac{1}{2}\\in \\mathbb{Q}", "\\text{true}"),
        ("\\sqrt{2}\\in \\mathbb{Q}", "\\text{false}"),
        (
            "\\sqrt{2}\\in \\mathbb{R}\\setminus \\mathbb{Q}",
            "\\text{true}",
        ),
        ("\\pi \\notin \\mathbb{Q}", "\\text{true}"),
        ("7\\in \\mathbb{P}", "\\text{true}"),
        ("2\\in \\left(0,2\\right]", "\\text{true}"),
        ("2\\in \\left[0,2\\right)", "\\text{false}"),
        ("1<2\\wedge 3>4", "\\text{false}"),
        ("1<2\\vee 3>4", "\\text{true}"),
        ("\\neg \\left(1>2\\right)", "\\text{true}"),
        ("1>2\\Rightarrow 5<0", "\\text{true}"),
        ("1<2\\Leftrightarrow 2<3", "\\text{true}"),
        ("3\\mid 12", "\\text{true}"),
        ("5\\nmid 12", "\\text{true}"),
        ("\\mathbb{N}\\subseteq \\mathbb{Z}", "\\text{true}"),
        ("\\mathbb{Q}\\subset \\mathbb{Z}", "\\text{false}"),
        ("\\mathbb{N}\\subsetneq \\mathbb{N}", "\\text{false}"),
        (
            "\\left[0,1\\right]\\subseteq \\left[0,2\\right)",
            "\\text{true}",
        ),
    ];
    for (latex, want) in cases {
        assert_eq!(verdict(&[latex]), want, "{latex}");
    }
}

#[test]
fn lists_as_sets() {
    let a = "A=\\left[1,2,3,4\\right]";
    let b = "B=\\left[3,4,5\\right]";
    let list = |latex: &str| match run(&[a, b, latex]).pop().unwrap().value {
        Some(Value::Numbers(v)) => v,
        other => panic!("{latex}: {other:?}"),
    };
    assert_eq!(list("A\\cup B"), vec![1.0, 2.0, 3.0, 4.0, 5.0]);
    assert_eq!(list("A\\cap B"), vec![3.0, 4.0]);
    assert_eq!(list("A\\setminus B"), vec![1.0, 2.0]);
    assert_eq!(verdict(&[a, b, "3\\in A"]), "\\text{true}");
    assert_eq!(verdict(&[a, b, "5\\notin A"]), "\\text{true}");
    assert_eq!(verdict(&[a, b, "A\\cap B\\subseteq B"]), "\\text{true}");
    assert_eq!(verdict(&[a, b, "A\\subseteq B"]), "\\text{false}");
    assert_eq!(verdict(&[a, b, "\\forall a\\in A:a<5"]), "\\text{true}");
    assert_eq!(
        verdict(&[a, b, "\\exists b\\in B:b>4"]),
        "\\text{true: }b=5"
    );
    assert_eq!(verdict(&[a, "\\varnothing \\subseteq A"]), "\\text{true}");
}

#[test]
fn undefined_names_are_offered_as_sliders() {
    let out = run(&["\\forall n\\in \\mathbb{N}:n>a"]);
    assert_eq!(out[0].missing, vec!["a".to_owned()]);
}

#[test]
fn restrictions_to_intervals_and_sets() {
    // An interval restricts a curve like a chain does.
    let out = run(&["y=x^2\\left\\{x\\in \\left[0,2\\right]\\right\\}"]);
    assert_eq!(out[0].error, None);
    let c = out[0].curve().expect("a curve");
    assert!(c.program.eval(1.0).is_finite());
    assert!(c.program.eval(3.0).is_nan());
    assert!(c.program.eval(-1.0).is_nan());
    // An open end leaves its point out.
    let out = run(&["y=x\\left\\{x\\in \\left(0,2\\right]\\right\\}"]);
    let c = out[0].curve().expect("a curve");
    assert!(c.program.eval(0.0).is_nan());
    assert!(c.program.eval(2.0).is_finite());
    // Discrete sets draw dots.
    let out = run(&["y=x^2\\left\\{x\\in \\mathbb{Z}\\right\\}\\left\\{-2\\le x\\le 2\\right\\}"]);
    assert_eq!(out[0].error, None);
    let mut pts: Vec<[f64; 2]> = out[0]
        .points
        .iter()
        .copied()
        .filter(|p| p[1].is_finite())
        .collect();
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    assert_eq!(
        pts,
        vec![[-2.0, 4.0], [-1.0, 1.0], [0.0, 0.0], [1.0, 1.0], [2.0, 4.0]]
    );
    let out = run(&["y=\\frac{1}{x}\\left\\{x\\in \\mathbb{N}\\right\\}"]);
    assert_eq!(out[0].error, None);
    assert_eq!(out[0].points.len(), 1000);
    let out = run(&["L=\\left[1,4\\right]", "y=x\\left\\{x\\in L\\right\\}"]);
    assert_eq!(out[1].points, vec![[1.0, 1.0], [4.0, 4.0]]);
    // A union of an interval and a number system.
    let out = run(&["y=1\\left\\{x\\in \\mathbb{R}\\setminus \\left[0,1\\right]\\right\\}"]);
    let c = out[0].curve().expect("a curve");
    assert!(c.program.eval(0.5).is_nan());
    assert!(c.program.eval(2.0).is_finite());
    // Logic inside braces.
    let out = run(&["y=x\\left\\{x<0\\vee x>1\\right\\}"]);
    let c = out[0].curve().expect("a curve");
    assert!(c.program.eval(0.5).is_nan());
    assert!(c.program.eval(-1.0).is_finite());
}

#[test]
fn friendly_errors() {
    let err = |latex: &str| run(&[latex]).pop().unwrap().error.unwrap_or_default();
    assert!(err("\\mathbb{R}").contains("set"), "{}", err("\\mathbb{R}"));
    assert!(err("\\forall n\\in \\mathbb{N}").contains("statement"));
    assert!(err("\\forall z\\in \\mathbb{C}:z=z").contains("ℂ"));
}

#[test]
fn sums_with_a_quantified_bound() {
    let v = verdict(&["\\forall n\\in\\mathbb{N}:\\sum_{k=1}^{n}k=\\frac{n\\left(n+1\\right)}{2}"]);
    assert!(v.starts_with("\\text{true}"), "{v}");
    assert_eq!(
        verdict(&["\\forall n\\in\\mathbb{N}:\\sum_{k=1}^{n}k^3=n^2"]),
        "\\text{false at }n=2"
    );
}

fn solve(latex: &str, others: &[&str]) -> String {
    let others: Vec<String> = others.iter().map(|s| (*s).into()).collect();
    let r = graph_expr::symbolic::run(latex, &others, None);
    assert_eq!(r.error, None, "{latex}");
    r.result.unwrap_or_else(|| panic!("no result for {latex}"))
}

#[test]
fn solve_in_a_set() {
    assert_eq!(solve("x^2=4,x\\in\\mathbb{N}", &[]), "x=2");
    // The restriction can sit in its own row.
    assert_eq!(solve("x^2=4", &["x\\in\\mathbb{N}"]), "x=2");
    let r = solve("\\sin x=0,x\\in\\left[0,2\\pi\\right]", &[]);
    assert!(
        r.contains("x=0") && r.contains("\\pi") && r.contains("2\\pi"),
        "{r}"
    );
    assert_eq!(
        solve("x^2+y^2=25,x,y\\in\\mathbb{N}", &[]),
        "\\left(x,y\\right)=\\left(3,4\\right),\\ \\left(4,3\\right)"
    );
    assert_eq!(
        solve("3x+5y=1,x,y\\in\\mathbb{Z}", &[]),
        "x=2+5t,\\ y=-1-3t\\quad\\left(t\\in\\mathbb{Z}\\right)"
    );
    assert_eq!(
        solve("6x+4y=3,x,y\\in\\mathbb{Z}", &[]),
        "\\text{no solution in }\\mathbb{Z}"
    );
}

#[test]
fn solve_proves_statements() {
    assert_eq!(
        solve("\\forall x\\in\\mathbb{R}:x^2+1>0", &[]),
        "\\text{true}"
    );
    assert_eq!(
        solve("\\forall x\\in\\mathbb{R}:\\sin ^2x+\\cos ^2x=1", &[]),
        "\\text{true}"
    );
    assert_eq!(
        solve(
            "\\forall n\\in\\mathbb{N}:\\sum_{k=1}^{n}k=\\frac{n\\left(n+1\\right)}{2}",
            &[]
        ),
        "\\text{true}"
    );
    assert!(solve("\\forall x:x^2>1", &[]).starts_with("\\text{false"));
    assert!(solve("\\exists x\\in\\mathbb{Q}:x^2=2", &[]).starts_with("\\text{false"));
    assert_eq!(
        solve("\\exists x\\in\\mathbb{R}:x^2=2", &[]),
        "\\text{true: }x=-\\sqrt{2},\\ x=\\sqrt{2}"
    );
}
