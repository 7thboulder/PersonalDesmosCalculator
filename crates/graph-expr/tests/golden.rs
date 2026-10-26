//! Golden tests: expressions exactly as MathQuill writes them, with the
//! result Desmos gives. This is the Phase 1 gate for matching Desmos syntax.

use graph_expr::{analyze, Axis, Compare, RowAnalysis, RowInput, RowKind, Value};

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
    let curve = last.curve().unwrap_or_else(|| panic!("no curve: {last:?}"));
    curve.program.eval(x)
}

fn value(latex: &[&str]) -> f64 {
    let all = rows(latex);
    let last = all.last().unwrap();
    last.number()
        .unwrap_or_else(|| panic!("no value: {last:?}"))
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
    let curve = all[0].curve().unwrap();
    assert_eq!(curve.axis, Axis::Y);
    assert!(close(curve.program.eval(3.0), 9.0));

    let vertical = rows(&["x=3"]);
    assert_eq!(vertical[0].curve().unwrap().axis, Axis::Y);
}

#[test]
fn variables_and_sliders() {
    let all = rows(&["a=2", "y=ax"]);
    assert_eq!(all[0].kind, RowKind::Variable("a".into()));
    assert_eq!(all[0].slider, Some(2.0));
    assert!(close(all[1].curve().unwrap().program.eval(3.0), 6.0));

    // Definitions can come after their use, and chain.
    let all = rows(&["y=bx", "b=2a", "a=-1.5"]);
    assert!(close(all[0].curve().unwrap().program.eval(2.0), -6.0));
    assert_eq!(all[1].slider, None);
    assert!(
        close(all[1].number().unwrap(), -3.0),
        "b = 2a shows its value"
    );
    assert_eq!(all[2].slider, Some(-1.5));

    // Subscripted names.
    assert!(close(curve_at(&["a_1=3", "y=a_1x"], 2.0), 6.0));
}

#[test]
fn undefined_names_offer_sliders() {
    let r = one("y=mx+b");
    assert_eq!(r.kind, RowKind::Curve);
    assert_eq!(r.missing, vec!["m".to_string(), "b".to_string()]);
    assert!(r.curve().is_none());
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
        close(all[0].curve().unwrap().program.eval(2.0), 5.0),
        "f(x)=... is graphed"
    );
    assert!(close(all[1].curve().unwrap().program.eval(3.0), 5.0));
    assert!(close(all[2].number().unwrap(), 10.0));

    // Two parameters, and a function of t that is not graphed.
    let all = rows(&[
        "g\\left(a,b\\right)=a-b",
        "g\\left(5,2\\right)",
        "h\\left(t\\right)=2t",
    ]);
    assert!(close(all[1].number().unwrap(), 3.0));
    assert!(all[2].curve().is_none() && all[2].error.is_none());

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
fn implicit_equations_and_inequalities() {
    // x^2 + y^2 = 25: the difference is zero exactly on the circle.
    let r = one("x^2+y^2=25");
    let region = r.region().expect("a region");
    assert_eq!(region.compare, Compare::Eq);
    assert!(close(region.program.eval2(3.0, 4.0), 0.0));
    assert!(close(region.program.eval2(0.0, 0.0), -25.0));

    // Inequalities keep the sense they were written with.
    let inside = one("x^2+y^2<9").region().cloned().unwrap();
    assert_eq!(inside.compare, Compare::Lt);
    assert!(inside.compare.holds(inside.program.eval2(0.0, 0.0)));
    assert!(!inside.compare.holds(inside.program.eval2(5.0, 0.0)));
    assert!(inside.compare.is_strict());

    // y > x^2 is an inequality, not a curve, and shades above the parabola.
    let above = one("y>x^2");
    assert!(above.curve().is_none());
    let above = above.region().cloned().unwrap();
    assert!(above.compare.holds(above.program.eval2(0.0, 1.0)));
    assert!(!above.compare.holds(above.program.eval2(0.0, -1.0)));
    assert!(above.compare.shades());

    // Sliders work inside an implicit equation, and both sides may be rich.
    let all = rows(&["a=2", "\\sin\\left(xy\\right)=\\cos\\left(ax\\right)"]);
    let r = all[1].region().unwrap();
    let f = |x: f64, y: f64| (x * y).sin() - (2.0 * x).cos();
    assert!(close(r.program.eval2(1.3, 0.7), f(1.3, 0.7)));

    // An undefined name still offers a slider instead of plotting.
    let r = one("x^2+y^2=k");
    assert_eq!(r.missing, vec!["k".to_string()]);
    assert!(r.region().is_none());
}

#[test]
fn calculus() {
    let pi = std::f64::consts::PI;
    // Derivatives, symbolic and evaluated wherever x is.
    assert!(close(curve_at(&["\\frac{d}{dx}x^3"], 2.0), 12.0));
    assert!(close(
        curve_at(&["y=\\frac{d}{dx}\\sin\\left(x^2\\right)"], 1.5),
        3.0 * (2.25f64).cos()
    ));
    let all = rows(&[
        "f\\left(x\\right)=x^3-2x",
        "y=f'\\left(x\\right)",
        "f''\\left(2\\right)",
        "\\frac{d}{dx}f\\left(x\\right)",
    ]);
    assert!(close(all[1].curve().unwrap().program.eval(2.0), 10.0));
    assert!(close(all[2].number().unwrap(), 12.0));
    assert!(close(all[3].curve().unwrap().program.eval(1.0), 1.0));
    // With respect to a slider: d/da of a x² is x².
    assert!(close(curve_at(&["a=3", "\\frac{d}{da}ax^2"], 2.0), 4.0));

    // Sums and products expand, so they graph like anything else.
    assert!(close(value(&["\\sum_{n=1}^{100}n"]), 5050.0));
    assert!(close(value(&["\\prod_{n=1}^{5}n"]), 120.0));
    assert!(
        close(value(&["\\sum_{n=1}^{3}n+1"]), 7.0),
        "the body stops at +"
    );
    let taylor = "y=\\sum_{n=0}^{10}\\frac{x^n}{n!}";
    assert!((curve_at(&[taylor], 1.0) - std::f64::consts::E).abs() < 1e-7);

    // Integrals, with constant or moving bounds, finite or not.
    assert!(close(value(&["\\int_{0}^{\\pi}\\sin tdt"]), 2.0));
    assert!(close(value(&["\\int_{0}^{\\infty}e^{-t}dt"]), 1.0));
    assert!(close(value(&["\\int_{0}^{1}t^2+1dt"]), 4.0 / 3.0));
    let area = "y=\\int_{0}^{x}\\cos tdt";
    assert!(close(curve_at(&[area], pi / 2.0), 1.0));
    // The derivative of an accumulation is the integrand.
    assert!(close(
        curve_at(&["\\frac{d}{dx}\\int_{0}^{x}t^2dt"], 3.0),
        9.0
    ));
    // A double integral.
    assert!(close(value(&["\\int_{0}^{1}\\int_{0}^{2}stdsdt"]), 1.0));
}

#[test]
fn empty_rows() {
    assert_eq!(one("").kind, RowKind::Empty);
    assert_eq!(one("  ").kind, RowKind::Empty);
}

fn row_with_domain(latex: &str, lo: &str, hi: &str) -> RowAnalysis {
    analyze(&[RowInput {
        id: 0,
        latex: latex.into(),
        domain: Some([lo.into(), hi.into()]),
        ..Default::default()
    }])
    .remove(0)
}

#[test]
fn points() {
    let r = one("\\left(1,2\\right)");
    assert_eq!(r.points, vec![[1.0, 2.0]]);
    assert_eq!(r.value, None, "a point written out shows no value");
    assert!(r.graphed());

    let all = rows(&[
        "p=\\left(1,2\\right)",
        "p.x+1",
        "\\left(1,2\\right)+\\left(3,4\\right)",
        "2p",
    ]);
    assert_eq!(all[0].points, vec![[1.0, 2.0]], "point variables are drawn");
    assert!(close(all[1].number().unwrap(), 2.0));
    assert_eq!(all[2].value, None, "points show on the graph instead");
    assert_eq!(all[2].points, vec![[4.0, 6.0]]);
    assert_eq!(all[3].points, vec![[2.0, 4.0]]);

    assert_eq!(
        one("\\left(1,2\\right)+3").error.as_deref(),
        Some("Points can't be added to numbers")
    );
    assert!(one("\\left(x,2\\right)").error.is_some());
}

#[test]
fn lists() {
    let numbers = |latex: &[&str]| match rows(latex).pop().unwrap().value {
        Some(Value::Numbers(v)) => v,
        other => panic!("not a list: {other:?}"),
    };
    assert_eq!(numbers(&["\\left[1,2,3\\right]^2"]), vec![1.0, 4.0, 9.0]);
    assert_eq!(numbers(&["\\left[1,3,...,11\\right]+0"]).len(), 6);
    assert_eq!(
        numbers(&["\\left[3...1\\right]+0"]),
        vec![3.0, 2.0, 1.0],
        "ranges count down"
    );
    assert_eq!(
        numbers(&["\\left[n^2\\operatorname{for}n=\\left[1...4\\right]\\right]"]),
        vec![1.0, 4.0, 9.0, 16.0]
    );
    assert_eq!(
        numbers(&[
            "L=\\left[1,2,3\\right]",
            "L\\left[\\left[1,3\\right]\\right]"
        ]),
        vec![1.0, 3.0]
    );
    assert_eq!(
        numbers(&["\\left[1,2,3\\right]+\\left[10,20\\right]"]),
        vec![11.0, 22.0],
        "the shorter list sets the length"
    );

    assert!(close(
        value(&["\\operatorname{total}\\left(\\left[1...10\\right]\\right)"]),
        55.0
    ));
    let all = rows(&[
        "L=\\left[1,2,3\\right]",
        "L\\left[2\\right]",
        "\\operatorname{mean}\\left(L\\right)",
        "\\operatorname{length}\\left(L\\right)",
        "\\max\\left(L\\right)",
        "L.\\operatorname{length}",
    ]);
    assert_eq!(all[0].value, None, "a written-out list shows no value");
    assert!(close(all[1].number().unwrap(), 2.0));
    assert!(close(all[2].number().unwrap(), 2.0));
    assert!(close(all[3].number().unwrap(), 3.0));
    assert!(close(all[4].number().unwrap(), 3.0));
    assert!(close(all[5].number().unwrap(), 3.0));

    // A list in a curve draws one curve per element.
    let all = rows(&["L=\\left[1,2,3\\right]", "y=Lx"]);
    assert_eq!(all[1].curves.len(), 3);
    assert!(close(all[1].curves[2].program.eval(2.0), 6.0));
    // And a list of points draws them all.
    let r = one("\\left[\\left(n,n^2\\right)\\operatorname{for}n=\\left[1...3\\right]\\right]");
    assert_eq!(r.points, vec![[1.0, 1.0], [2.0, 4.0], [3.0, 9.0]]);
}

#[test]
fn parametric_and_polar() {
    let r = one("\\left(\\cos t,\\sin t\\right)");
    assert_eq!(r.parametrics.len(), 1);
    let p = &r.parametrics[0];
    assert_eq!(p.domain, (0.0, 1.0), "Desmos's default domain");
    assert!(close(p.x.eval(0.0), 1.0) && close(p.y.eval(0.0), 0.0));
    assert_eq!(r.parameter.as_ref().unwrap().name, "t");

    let r = row_with_domain("\\left(\\cos t,\\sin t\\right)", "0", "2\\pi");
    assert!(close(r.parametrics[0].domain.1, std::f64::consts::TAU));
    let r = row_with_domain("\\left(\\cos t,\\sin t\\right)", "0", "q");
    assert!(
        r.parameter.unwrap().error.is_some(),
        "a bad bound is reported"
    );
    assert_eq!(r.parametrics[0].domain, (0.0, 1.0), "and ignored");

    // A defined t makes the same row a point.
    let all = rows(&["t=0", "\\left(\\cos t,\\sin t\\right)"]);
    assert_eq!(all[1].points, vec![[1.0, 0.0]]);

    let r = one("r=\\theta");
    let p = &r.parametrics[0];
    let pi = std::f64::consts::PI;
    assert!(close(p.x.eval(pi), -pi) && p.y.eval(pi).abs() < 1e-12);
    assert!(close(p.domain.1, 12.0 * pi));
    assert_eq!(one("r=2").parametrics.len(), 1, "a circle");

    // r and θ inside a relation are measured from the origin.
    let disc = one("r<2").region().cloned().unwrap();
    assert!(disc.program.eval2(1.0, 1.0) < 0.0);
    assert!(disc.program.eval2(2.0, 1.0) > 0.0);
    let flower = one("r<2+\\sin\\left(3\\theta\\right)")
        .region()
        .cloned()
        .unwrap();
    assert!(flower.program.eval2(0.5, 0.5) < 0.0);
}

#[test]
fn piecewise_and_restrictions() {
    let curve = |latex: &str, x: f64| curve_at(&[latex], x);
    assert!(close(curve("y=\\left\\{x<0:-x,x\\right\\}", -3.0), 3.0));
    assert!(close(curve("y=\\left\\{x<0:-x,x\\right\\}", 2.0), 2.0));
    assert!(curve("y=x^2\\left\\{x>0\\right\\}", -1.0).is_nan());
    assert!(close(curve("y=x^2\\left\\{x>0\\right\\}", 2.0), 4.0));
    assert!(curve("y=x\\left\\{0<x<2\\right\\}", 3.0).is_nan());
    assert!(close(curve("y=x\\left\\{0<x<2\\right\\}", 1.0), 1.0));
    assert!(
        curve("y=\\left\\{x>0:1\\right\\}", -1.0).is_nan(),
        "no else"
    );
    assert!(close(value(&["\\left\\{1>0:5,6\\right\\}"]), 5.0));
    assert!(close(value(&["\\left\\{1<0:5,6\\right\\}"]), 6.0));

    // A chain shades where every comparison holds.
    let band = one("-1<y<x").region().cloned().unwrap();
    assert!(band.compare.holds(band.program.eval2(2.0, 0.0)));
    assert!(!band.compare.holds(band.program.eval2(0.0, 5.0)));
    assert!(!band.compare.holds(band.program.eval2(5.0, -3.0)));
}

fn table(headers: &[&str], cells: &[&[&str]]) -> RowInput {
    RowInput {
        table: Some(
            headers
                .iter()
                .enumerate()
                .map(|(c, h)| graph_expr::ColumnInput {
                    header: h.to_string(),
                    cells: cells
                        .iter()
                        .map(|r| r.get(c).unwrap_or(&"").to_string())
                        .collect(),
                })
                .collect(),
        ),
        ..Default::default()
    }
}

#[test]
fn tables() {
    let with = |extra: &[&str], t: RowInput| {
        let mut input = vec![RowInput {
            id: 0,
            table: t.table,
            ..Default::default()
        }];
        for (i, l) in extra.iter().enumerate() {
            input.push(RowInput {
                id: i as u64 + 1,
                latex: l.to_string(),
                ..Default::default()
            });
        }
        analyze(&input)
    };
    // Points against the first column; the blank row at the end is ignored
    // and a blank cell skips its point.
    let t = table(
        &["x_1", "y_1"],
        &[&["1", "2"], &["2", ""], &["3", "a"], &["", ""]],
    );
    let all = with(
        &[
            "a=5",
            "\\operatorname{total}\\left(x_1\\right)",
            "y_1\\left[3\\right]",
        ],
        t,
    );
    assert_eq!(all[0].kind, RowKind::Table);
    assert_eq!(all[0].columns[1].points, vec![[1.0, 2.0], [3.0, 5.0]]);
    assert!(close(all[2].number().unwrap(), 6.0));
    assert!(close(all[3].number().unwrap(), 5.0));

    // A formula column is filled in from the first column, even one headed x.
    let t = table(&["x", "x^2", "y"], &[&["1", "", "4"], &["-2", "", "5"]]);
    let all = with(&[], t);
    assert_eq!(all[0].columns[1].computed, Some(vec![1.0, 4.0]));
    assert_eq!(all[0].columns[1].points, vec![[1.0, 1.0], [-2.0, 4.0]]);
    assert_eq!(all[0].columns[2].points, vec![[1.0, 4.0], [-2.0, 5.0]]);

    // Undefined names are offered as sliders; reused names are errors.
    let t = table(&["x_1", "y_1"], &[&["1", "b"]]);
    let all = with(&["y_1=3"], t);
    assert_eq!(
        all[0].columns[1].error.as_deref(),
        Some("'y_1' is defined more than once")
    );
    let t = table(&["x_1", "y_2"], &[&["1", "b"]]);
    assert_eq!(with(&[], t)[0].missing, vec!["b".to_string()]);

    // Bad cells and headers say what is wrong.
    let t = table(&["x_1", "y_1"], &[&["1", "2+"]]);
    assert_eq!(with(&[], t)[0].columns[1].cell_errors.len(), 1);
    let t = table(&["x_1^2", "y_1"], &[&["1", "2"]]);
    assert!(with(&[], t)[0].columns[0].error.is_some());
}

#[test]
fn x_and_y_can_name_functions() {
    // Desmos lets x(t) and y(t) be defined for a parametric (x(t), y(t)).
    let all = rows(&[
        "x\\left(t\\right)=4\\sin t",
        "y\\left(t\\right)=3\\cos t",
        "\\left(x\\left(t\\right),y\\left(t\\right)\\right)",
        "x\\left(0.5\\right)",
    ]);
    assert_eq!(all[0].kind, RowKind::Function("x".into()));
    assert!(all[0].missing.is_empty() && all[0].error.is_none());
    let p = &all[2].parametrics[0];
    assert!(close(p.x.eval(0.3), 4.0 * 0.3f64.sin()));
    assert!(close(p.y.eval(0.3), 3.0 * 0.3f64.cos()));
    assert!(close(all[3].number().unwrap(), 4.0 * 0.5f64.sin()));
    // Bare x is still the graph variable.
    assert!(close(curve_at(&["x\\left(t\\right)=t", "y=x^2"], 3.0), 9.0));
}
