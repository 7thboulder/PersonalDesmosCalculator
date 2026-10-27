//! Matrix rows and the rows that use them, typed as MathQuill writes them.

use graph_expr::{analyze, Ink, Matrix, RowAnalysis, RowInput, RowKind, Value};

/// A matrix row named `name` with the given cells.
fn grid(name: &str, cells: &[&[&str]]) -> RowInput {
    RowInput {
        latex: name.into(),
        matrix: Some(
            cells
                .iter()
                .map(|r| r.iter().map(|c| c.to_string()).collect())
                .collect(),
        ),
        ..Default::default()
    }
}

fn row(latex: &str) -> RowInput {
    RowInput {
        latex: latex.into(),
        ..Default::default()
    }
}

fn run(mut input: Vec<RowInput>) -> Vec<RowAnalysis> {
    for (i, r) in input.iter_mut().enumerate() {
        r.id = i as u64;
    }
    analyze(&input)
}

fn a() -> RowInput {
    grid("A", &[&["1", "2"], &["3", "4"]])
}

fn matrix(r: &RowAnalysis) -> Vec<Vec<f64>> {
    let m = r
        .matrix
        .as_ref()
        .unwrap_or_else(|| panic!("no matrix: {r:?}"));
    (0..m.rows)
        .map(|i| (0..m.cols).map(|j| m.get(i, j)).collect())
        .collect()
}

#[test]
fn a_matrix_row_defines_its_name() {
    let out = run(vec![a(), row("\\det\\left(A\\right)")]);
    assert_eq!(out[0].kind, RowKind::Variable("A".into()));
    assert_eq!(matrix(&out[0]), vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
    assert_eq!(out[1].value, Some(Value::Number(-2.0)));
}

#[test]
fn matrix_functions_and_arithmetic() {
    let out = run(vec![
        a(),
        row("A^{-1}"),
        row("A^T"),
        row("A^2"),
        row("\\operatorname{rref}\\left(A\\right)"),
        row("\\operatorname{trace}\\left(A\\right)"),
        row("\\operatorname{rank}\\left(A\\right)"),
        row("2A-A"),
        row("A\\cdot\\operatorname{inv}\\left(A\\right)"),
        row("\\operatorname{transpose}\\left(A\\right)"),
        row("\\operatorname{identity}\\left(2\\right)"),
        row("A\\left[2,1\\right]"),
    ]);
    assert_eq!(matrix(&out[1]), vec![vec![-2.0, 1.0], vec![1.5, -0.5]]);
    assert_eq!(matrix(&out[2]), vec![vec![1.0, 3.0], vec![2.0, 4.0]]);
    assert_eq!(matrix(&out[3]), vec![vec![7.0, 10.0], vec![15.0, 22.0]]);
    assert_eq!(matrix(&out[4]), vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
    assert_eq!(out[5].value, Some(Value::Number(5.0)));
    assert_eq!(out[6].value, Some(Value::Number(2.0)));
    assert_eq!(matrix(&out[7]), matrix(&out[0]));
    assert_eq!(matrix(&out[8]), vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
    assert_eq!(matrix(&out[9]), matrix(&out[2]));
    assert_eq!(matrix(&out[10]), vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
    assert_eq!(out[11].value, Some(Value::Number(3.0)));
}

#[test]
fn defined_matrices_and_numbers_from_matrices() {
    let out = run(vec![
        a(),
        row("B=A^{-1}"),
        row("d=\\det\\left(A\\right)"),
        row("y=dx"),
        row("BA"),
        row("f\\left(x\\right)=\\det\\left(A\\right)x^2"),
        row("f\\left(3\\right)"),
    ]);
    assert_eq!(out[1].kind, RowKind::Variable("B".into()));
    assert_eq!(matrix(&out[1]), vec![vec![-2.0, 1.0], vec![1.5, -0.5]]);
    assert_eq!(out[2].value, Some(Value::Number(-2.0)));
    assert_eq!(out[2].kind, RowKind::Variable("d".into()));
    assert!(out[3].error.is_none(), "{:?}", out[3].error);
    assert_eq!(out[3].curve().unwrap().program.eval(2.0), -4.0);
    assert_eq!(matrix(&out[4]), vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
    assert_eq!(out[6].value, Some(Value::Number(-18.0)));
}

#[test]
fn cells_read_sliders_and_offer_missing_ones() {
    let out = run(vec![
        grid("M", &[&["a", "0"], &["0", "\\sqrt{4}"]]),
        row("a=3"),
        row("\\det\\left(M\\right)"),
    ]);
    assert_eq!(matrix(&out[0]), vec![vec![3.0, 0.0], vec![0.0, 2.0]]);
    assert_eq!(out[2].value, Some(Value::Number(6.0)));

    let out = run(vec![grid("M", &[&["k", ""], &["0", "1"]])]);
    assert!(out[0].error.is_some());
    assert_eq!(out[0].missing, vec!["k".to_string()]);
}

#[test]
fn matrices_move_points_and_curves() {
    let out = run(vec![
        grid("R", &[&["0", "-1"], &["1", "0"]]),
        row("R\\left(1,2\\right)"),
        row("R\\left(\\cos t,2\\sin t\\right)"),
        row("p=\\left(3,4\\right)"),
        row("Rp"),
    ]);
    assert_eq!(out[1].points, vec![[-2.0, 1.0]]);
    let curve = &out[2].parametrics[0];
    assert!((curve.x.eval(0.0) - 0.0).abs() < 1e-12);
    assert!((curve.y.eval(0.0) - 1.0).abs() < 1e-12);
    assert_eq!(out[4].points, vec![[-4.0, 3.0]]);
}

#[test]
fn written_out_matrices() {
    let out = run(vec![
        row("C=\\left[\\left[1,2,3\\right],\\left[4,5,6\\right]\\right]"),
        row("C^T"),
        row("\\operatorname{rank}\\left(C\\right)"),
    ]);
    assert_eq!(
        matrix(&out[0]),
        vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]
    );
    assert_eq!(matrix(&out[1]).len(), 3);
    assert_eq!(out[2].value, Some(Value::Number(2.0)));
}

#[test]
fn eigenvalues_and_eigenvectors() {
    let out = run(vec![
        grid("A", &[&["2", "1"], &["1", "2"]]),
        row("\\operatorname{eigenvalues}\\left(A\\right)"),
        row("\\operatorname{eigenvectors}\\left(A\\right)"),
        grid("R", &[&["0", "-1"], &["1", "0"]]),
        row("\\operatorname{eigenvalues}\\left(R\\right)"),
    ]);
    match &out[1].value {
        Some(Value::Numbers(v)) => {
            assert!(
                (v[0] - 3.0).abs() < 1e-9 && (v[1] - 1.0).abs() < 1e-9,
                "{v:?}"
            )
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(matrix(&out[2]), vec![vec![1.0, -1.0], vec![1.0, 1.0]]);
    // Each eigenvector draws as an arrow on its line.
    assert!(out[2].picture.iter().any(|s| s.ink == Ink::Eigen));
    assert!(out[2].graphed());
    match &out[4].value {
        Some(Value::Complexes(v)) => assert!(v.iter().all(|z| (z[1].abs() - 1.0).abs() < 1e-9)),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_transformation_view() {
    let mut input = vec![a()];
    input[0].transform = Some(0.5);
    let out = run(input);
    assert!(out[0].error.is_none());
    let tip = out[0]
        .picture
        .iter()
        .find(|s| s.ink == Ink::IHat && s.filled)
        .unwrap()
        .points[0];
    // Halfway from (1, 0) to A's first column (1, 3).
    assert_eq!(tip, [1.0, 1.5]);
    // Not on unless asked for.
    assert!(run(vec![a()])[0].picture.is_empty());
}

#[test]
fn matrix_mistakes_say_why() {
    let message = |rows: Vec<RowInput>| {
        let out = run(rows);
        out.last().unwrap().error.clone().unwrap_or_default()
    };
    let wide = || grid("W", &[&["1", "2", "3"], &["4", "5", "6"]]);
    assert!(message(vec![wide(), row("\\det\\left(W\\right)")]).contains("square"));
    assert!(message(vec![a(), wide(), row("AW")]).is_empty());
    assert!(message(vec![a(), wide(), row("WA")]).contains("multiply"));
    assert!(
        message(vec![grid("S", &[&["1", "2"], &["2", "4"]]), row("S^{-1}")]).contains("no inverse")
    );
    assert!(message(vec![a(), row("A+1")]).contains("added"));
    assert!(message(vec![row("\\det\\left(3\\right)")]).contains("needs a matrix"));
    assert!(message(vec![grid("", &[&["1"]])]).contains("Name"));
    assert!(message(vec![a(), grid("A", &[&["1"]])]).contains("more than once"));
    assert!(message(vec![a(), row("y=A")]).contains("compared"));
    let _ = Matrix::identity(1);
}
