//! Fractal rows: `mandelbrot()`, `julia(c)`, `cobweb(f, x_0, n)` and
//! `bifurcation(f, x_0)`, as MathQuill writes them.

use graph_expr::{analyze, Fractal, RowAnalysis, RowInput};

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

fn last(latex: &[&str]) -> RowAnalysis {
    rows(latex).pop().unwrap()
}

#[test]
fn mandelbrot_with_and_without_iterations() {
    let row = last(&["\\operatorname{mandelbrot}\\left(\\right)"]);
    assert_eq!(row.error, None);
    assert_eq!(row.fractals, vec![Fractal::Mandelbrot { iterations: None }]);
    assert!(row.graphed());
    let row = last(&["n=500", "\\operatorname{mandelbrot}\\left(n\\right)"]);
    assert_eq!(
        row.fractals,
        vec![Fractal::Mandelbrot {
            iterations: Some(500)
        }]
    );
    let row = last(&["\\operatorname{mandelbrot}\\left(0\\right)"]);
    assert!(row.error.is_some() && row.fractals.is_empty());
}

#[test]
fn julia_takes_a_complex_number_or_a_point() {
    let row = last(&["\\operatorname{julia}\\left(-0.8+0.156i\\right)"]);
    assert_eq!(row.error, None);
    assert_eq!(
        row.fractals,
        vec![Fractal::Julia {
            c: [-0.8, 0.156],
            iterations: None
        }]
    );
    let row = last(&["\\operatorname{julia}\\left(\\left(0.3,0.5\\right),100\\right)"]);
    assert_eq!(
        row.fractals,
        vec![Fractal::Julia {
            c: [0.3, 0.5],
            iterations: Some(100)
        }]
    );
    // An undefined name is offered as a slider.
    let row = last(&["\\operatorname{julia}\\left(a+bi\\right)"]);
    assert_eq!(row.missing, vec!["a", "b"]);
    assert!(row.fractals.is_empty());
}

#[test]
fn cobweb_steps_between_the_curve_and_y_equals_x() {
    let row = last(&[
        "f\\left(x\\right)=kx\\left(1-x\\right)",
        "k=2",
        "\\operatorname{cobweb}\\left(f,0.2,3\\right)",
    ]);
    assert_eq!(row.error, None);
    assert_eq!(row.paths.len(), 1);
    let path = &row.paths[0];
    // From the axis up to the curve, across to y = x, three times.
    assert_eq!(path.len(), 7);
    assert_eq!(path[0], [0.2, 0.0]);
    let f = |x: f64| 2.0 * x * (1.0 - x);
    let x1 = f(0.2);
    assert!((path[1][1] - x1).abs() < 1e-12 && path[1][0] == 0.2);
    assert_eq!(path[2], [path[1][1], path[1][1]]);
    assert!((path[6][0] - f(f(x1))).abs() < 1e-12);
}

#[test]
fn cobweb_from_a_list_draws_one_path_each() {
    let row = last(&[
        "g\\left(x\\right)=\\cos\\left(x\\right)",
        "\\operatorname{cobweb}\\left(g,\\left[0,1,2\\right]\\right)",
    ]);
    assert_eq!(row.error, None);
    assert_eq!(row.paths.len(), 3);
    assert_eq!(row.paths[0].len(), 41, "20 steps by default");
}

#[test]
fn bifurcation_reads_r_then_x() {
    let row = last(&[
        "f\\left(r,x\\right)=rx\\left(1-x\\right)",
        "\\operatorname{bifurcation}\\left(f\\right)",
    ]);
    assert_eq!(row.error, None);
    let [Fractal::Bifurcation { program, start }] = &row.fractals[..] else {
        panic!("{row:?}");
    };
    assert_eq!(*start, 0.5);
    assert!((program.eval2(3.0, 0.5) - 0.75).abs() < 1e-12);
}

#[test]
fn fractal_rows_explain_mistakes() {
    let error = |latex: &[&str]| last(latex).error.unwrap_or_default();
    assert!(error(&["\\operatorname{cobweb}\\left(h,0.2\\right)"]).contains("isn't a function"));
    assert!(error(&[
        "f\\left(x\\right)=x^2",
        "\\operatorname{bifurcation}\\left(f\\right)"
    ])
    .contains("two arguments"));
    assert!(error(&["1+\\operatorname{mandelbrot}\\left(\\right)"]).contains("row of its own"));
}
