//! Golden tests: expressions exactly as MathQuill writes them, with the
//! result Desmos gives. This is the Phase 1 gate for matching Desmos syntax.

use graph_expr::{
    analyze, analyze_cached, Axis, Cache, Compare, Ode, RowAnalysis, RowInput, RowKind, Value,
};

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
fn draggable_points() {
    let all = rows(&[
        "a=1",
        "b=2",
        "\\left(a,b\\right)",
        "P=\\left(a,3\\right)",
        "\\left(a+1,b\\right)",
        "\\left(2,3\\right)",
        "c=a+1",
        "\\left(c,b\\right)",
    ]);
    let s = |n: &str| Some(n.to_string());
    assert_eq!(all[2].drag, Some([s("a"), s("b")]));
    assert_eq!(all[3].drag, Some([s("a"), None]));
    // Only bare slider variables drag their coordinate.
    assert_eq!(all[4].drag, Some([None, s("b")]));
    assert_eq!(all[5].drag, None);
    assert_eq!(all[7].drag, Some([None, s("b")]));
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

#[test]
fn polygons() {
    let square = "\\operatorname{polygon}\\left(\\left(0,0\\right),\\left(1,0\\right),\\left(1,1\\right),\\left(0,1\\right)\\right)";
    let r = one(square);
    assert_eq!(r.error, None);
    assert_eq!(
        r.polygons,
        vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]]
    );
    assert!(r.graphed() && r.value.is_none());
    // From a list of points, from lists of coordinates, and one per element.
    let all = rows(&[
        "L=\\left[\\left(0,0\\right),\\left(2,0\\right),\\left(0,a\\right)\\right]",
        "a=3",
        "\\operatorname{polygon}\\left(L\\right)",
        "\\operatorname{polygon}\\left(\\left[0,2,0\\right],\\left[0,0,3\\right]\\right)",
        "\\operatorname{polygon}\\left(\\left(0,0\\right),\\left(1,0\\right),\\left(\\left[1,2\\right],1\\right)\\right)",
        "P=\\operatorname{polygon}\\left(L\\right)",
        "\\operatorname{length}\\left(\\left[P,P\\right]\\right)",
    ]);
    let triangle = vec![vec![[0.0, 0.0], [2.0, 0.0], [0.0, 3.0]]];
    assert_eq!(all[2].polygons, triangle);
    assert_eq!(all[3].polygons, triangle);
    assert_eq!(all[4].polygons.len(), 2);
    assert_eq!(all[4].polygons[1][2], [2.0, 1.0]);
    assert_eq!(all[5].polygons, triangle, "a polygon variable is drawn");
    assert_eq!(all[6].number(), Some(2.0));
    let bad = one("\\operatorname{polygon}\\left(\\left(x,0\\right),\\left(1,0\\right),\\left(1,1\\right)\\right)");
    assert!(bad.error.is_some() && bad.polygons.is_empty());
}

#[test]
fn list_functions() {
    let numbers = |latex: &str| match one(latex).value {
        Some(Value::Numbers(n)) => n,
        other => panic!("{latex}: {other:?}"),
    };
    assert_eq!(
        numbers("\\operatorname{join}\\left(\\left[1,2\\right],3,\\left[4\\right]\\right)"),
        vec![1.0, 2.0, 3.0, 4.0]
    );
    assert_eq!(
        numbers("\\operatorname{sort}\\left(\\left[3,1,2\\right]\\right)"),
        vec![1.0, 2.0, 3.0]
    );
    assert_eq!(
        numbers("\\operatorname{sort}\\left(\\left[10,20,30\\right],\\left[2,3,1\\right]\\right)"),
        vec![30.0, 10.0, 20.0]
    );
    assert_eq!(
        numbers("\\operatorname{unique}\\left(\\left[1,2,1,3,2\\right]\\right)"),
        vec![1.0, 2.0, 3.0]
    );
    assert!(close(
        value(&["\\operatorname{distance}\\left(\\left(0,0\\right),\\left(3,4\\right)\\right)"]),
        5.0
    ));
    let mid = one("\\operatorname{midpoint}\\left(\\left(0,0\\right),\\left(2,4\\right)\\right)");
    assert_eq!(mid.points, vec![[1.0, 2.0]]);
}

#[test]
fn actions() {
    let all = rows(&[
        "a=1",
        "b=5",
        "a\\to a+1",
        "a\\to b,b\\to a",
        "A=\\left(a\\to0,b\\to a\\right)",
        "A",
        "\\left\\{a>3:a\\to0,a\\to a+1\\right\\}",
        "L=\\left[1,2\\right]",
        "L\\to\\operatorname{join}\\left(L,\\left[a\\right]\\right)",
        "c\\to1",
        "a+A",
    ]);
    assert_eq!(all[2].kind, RowKind::Action);
    assert_eq!(all[2].updates, vec![("a".to_owned(), Value::Number(2.0))]);
    // Both sides use the old values, so this swaps a and b.
    assert_eq!(
        all[3].updates,
        vec![
            ("a".to_owned(), Value::Number(5.0)),
            ("b".to_owned(), Value::Number(1.0))
        ]
    );
    assert_eq!(all[4].kind, RowKind::Action, "a named action");
    assert_eq!(all[5].updates, all[4].updates, "running an action by name");
    assert_eq!(all[6].updates, vec![("a".to_owned(), Value::Number(2.0))]);
    assert_eq!(
        all[8].updates,
        vec![("L".to_owned(), Value::Numbers(vec![1.0, 2.0, 1.0]))]
    );
    assert_eq!(all[9].missing, vec!["c".to_owned()]);
    assert!(all[10].error.as_deref().unwrap().contains("action"));
}

/// Results kept from one analysis to the next must never differ from
/// working everything out afresh, whatever changed in between.
#[test]
fn cached_analyses_match_fresh_ones() {
    let input = |latex: &[&str]| -> Vec<RowInput> {
        latex
            .iter()
            .enumerate()
            .map(|(i, l)| RowInput {
                id: i as u64,
                latex: l.to_string(),
                ..Default::default()
            })
            .collect()
    };
    let steps: &[&[&str]] = &[
        &[
            "a=1",
            "L=[1,2,3]",
            "f(n)=\\sum_{k=1}^{n}L[k]a^k",
            "b=f(3)",
            "g(t)=\\sum_{n=1}^{3}f(n)e^{i\\cdot t}",
            "z=g(2)",
            "y=f(2)x",
        ],
        // A variable a function reads changes.
        &[
            "a=2",
            "L=[1,2,3]",
            "f(n)=\\sum_{k=1}^{n}L[k]a^k",
            "b=f(3)",
            "g(t)=\\sum_{n=1}^{3}f(n)e^{i\\cdot t}",
            "z=g(2)",
            "y=f(2)x",
        ],
        // A list it reads changes.
        &[
            "a=2",
            "L=[1,5,3]",
            "f(n)=\\sum_{k=1}^{n}L[k]a^k",
            "b=f(3)",
            "g(t)=\\sum_{n=1}^{3}f(n)e^{i\\cdot t}",
            "z=g(2)",
            "y=f(2)x",
        ],
        // The function itself changes.
        &[
            "a=2",
            "L=[1,5,3]",
            "f(n)=\\sum_{k=1}^{n}L[k]+a",
            "b=f(3)",
            "g(t)=\\sum_{n=1}^{3}f(n)e^{i\\cdot t}",
            "z=g(2)",
            "y=f(2)x",
        ],
        // A name it read stops being defined, then i becomes a variable.
        &[
            "L=[1,5,3]",
            "f(n)=\\sum_{k=1}^{n}L[k]+a",
            "b=f(3)",
            "g(t)=\\sum_{n=1}^{3}f(n)e^{i\\cdot t}",
            "z=g(2)",
        ],
        &[
            "a=2",
            "i=3",
            "L=[1,5,3]",
            "f(n)=\\sum_{k=1}^{n}L[k]+a",
            "b=f(3)",
            "g(t)=\\sum_{n=1}^{3}f(n)e^{i\\cdot t}",
            "z=g(2)",
        ],
    ];
    let mut cache = Cache::default();
    for (n, step) in steps.iter().enumerate() {
        let rows = input(step);
        let cached = analyze_cached(&rows, &mut cache);
        assert_eq!(cached, analyze(&rows), "step {n}");
    }
}

#[test]
fn cached_rows_match_fresh_ones() {
    // Expression rows are remembered by id between analyses; each step
    // changes something one of them reads.
    let input = |latex: &[&str]| -> Vec<RowInput> {
        latex
            .iter()
            .enumerate()
            .map(|(i, l)| RowInput {
                id: i as u64,
                latex: l.to_string(),
                ..Default::default()
            })
            .collect()
    };
    let rows = [
        "a=2",
        "L=[1...50000]",
        "(L/1000,a\\sin(L/1000))",
        "\\operatorname{total}(L)",
        "y=ax+b",
        "(t,at)",
        "\\operatorname{mandelbrot}()",
    ];
    let steps: Vec<Vec<&str>> = vec![
        rows.to_vec(),
        // Nothing changes.
        rows.to_vec(),
        // A slider some rows read.
        [&["a=3"], &rows[1..]].concat(),
        // The list.
        [&["a=3", "L=[1...40000]"], &rows[2..]].concat(),
        // A missing name becomes defined, and so does t.
        [&rows[..], &["b=1", "t=2"]].concat(),
        // Rows move to other ids.
        [&["c=1"], &rows[..]].concat(),
    ];
    let mut cache = Cache::default();
    for (n, step) in steps.iter().enumerate() {
        let rows = input(step);
        let cached = analyze_cached(&rows, &mut cache);
        assert_eq!(cached, analyze(&rows), "step {n}");
    }
}

#[test]
fn lists_can_be_longer_than_desmos_allows() {
    let all = rows(&[
        "L=[1...25000]",
        "\\operatorname{length}(L)",
        "\\operatorname{length}([a+b\\operatorname{for}a=[1...200],b=[1...200]])",
        "\\operatorname{length}(\\operatorname{join}(L,L))",
        "[1...10^{15}]",
    ]);
    assert_eq!(all[1].value, Some(Value::Number(25000.0)));
    assert_eq!(all[2].value, Some(Value::Number(40000.0)));
    assert_eq!(all[3].value, Some(Value::Number(50000.0)));
    assert_eq!(
        all[4].error.as_deref(),
        Some("There isn't enough memory for a list of 1e15 elements")
    );
}

#[test]
fn regressions() {
    let all = rows(&[
        "x_1=[1,2,3,4,5]",
        "y_1=[3,5,7,9,11]",
        "y_1\\sim mx_1+b",
        "c=m+b",
    ]);
    let fit = all[2]
        .fit
        .as_ref()
        .unwrap_or_else(|| panic!("{:?}", all[2]));
    assert_eq!(all[2].kind, RowKind::Regression);
    let param = |n: &str| fit.params.iter().find(|(p, _)| p == n).unwrap().1;
    assert!(close(param("m"), 2.0) && close(param("b"), 1.0), "{fit:?}");
    assert!(close(fit.r2.unwrap(), 1.0));
    assert_eq!(fit.residuals.as_deref(), Some("e_1"));
    // The model is drawn as y = 2x + 1, and the parameters are defined.
    assert!(close(all[2].curves[0].program.eval(10.0), 21.0));
    assert_eq!(all[3].value, Some(Value::Number(param("m") + param("b"))));
    assert!(all[3].missing.is_empty());

    // A model that is not linear in its parameters.
    let all = rows(&[
        "x_1=[0,1,2,3]",
        "y_1=[2,2e,2e^2,2e^3]",
        "y_1\\sim ae^{kx_1}",
    ]);
    let fit = all[2].fit.as_ref().unwrap();
    let param = |n: &str| fit.params.iter().find(|(p, _)| p == n).unwrap().1;
    assert!(
        (param("a") - 2.0).abs() < 1e-6 && (param("k") - 1.0).abs() < 1e-6,
        "{fit:?}"
    );

    // Noisy data fits less than perfectly.
    let all = rows(&["x_1=[1,2,3,4]", "y_1=[1,3,2,4]", "y_1\\sim mx_1+b"]);
    let fit = all[2].fit.as_ref().unwrap();
    let param = |n: &str| fit.params.iter().find(|(p, _)| p == n).unwrap().1;
    // Least squares: m = 0.8, b = 0.5, R² = 0.64.
    assert!(
        (param("m") - 0.8).abs() < 1e-9 && (param("b") - 0.5).abs() < 1e-9,
        "{fit:?}"
    );
    assert!((fit.r2.unwrap() - 0.64).abs() < 1e-9);
}

#[test]
fn list_filters() {
    let list = |latex: &[&str]| rows(latex).pop().unwrap().value;
    let l = "L=\\left[1,-2,3,-4,5\\right]";
    assert_eq!(
        list(&[l, "L\\left[L>0\\right]"]),
        Some(Value::Numbers(vec![1.0, 3.0, 5.0]))
    );
    assert_eq!(
        list(&[l, "L\\left[1<L\\le5\\right]"]),
        Some(Value::Numbers(vec![3.0, 5.0]))
    );
    // The condition can read another list, a slider, or nothing at all.
    assert_eq!(
        list(&[l, "a=2", "L\\left[\\left[1,2,3,4,5\\right]>a\\right]"]),
        Some(Value::Numbers(vec![3.0, -4.0, 5.0]))
    );
    assert_eq!(
        list(&[l, "L\\left[1>2\\right]"]),
        Some(Value::Numbers(vec![]))
    );
    assert!(close(
        value(&[l, "\\operatorname{total}\\left(L\\left[L<0\\right]\\right)"]),
        -6.0
    ));
    assert!(close(
        value(&[l, "L\\left[L>0\\right]\\left[2\\right]"]),
        3.0
    ));
    // Plain indices still index.
    assert!(close(value(&[l, "L\\left[2\\right]"]), -2.0));
    assert!(rows(&["a=3", "a\\left[a>1\\right]"])[1].error.is_some());
}

#[test]
fn long_sums_are_compiled_exactly() {
    // Long sums go through the compiled path; short ones term by term. Both
    // must give what the formula does, for real and complex terms, nested
    // sums, inlined functions and list elements.
    let expect: f64 = (1..=50).map(|n| (n as f64).sin() / n as f64).sum();
    assert!(close(
        value(&["\\sum_{n=1}^{50}\\frac{\\sin\\left(n\\right)}{n}"]),
        expect
    ));
    let all = rows(&[
        "L=\\left[1...40\\right]",
        "f\\left(k\\right)=L\\left[k\\right]^2",
        "\\sum_{k=1}^{40}f\\left(k\\right)",
        "\\sum_{j=1}^{10}\\sum_{k=1}^{j}k",
        "\\operatorname{real}\\left(\\sum_{n=0}^{11}e^{\\frac{2\\pi in}{12}}\\right)",
    ]);
    assert_eq!(
        all[2].number(),
        Some((1..=40).map(|k| (k * k) as f64).sum::<f64>())
    );
    assert_eq!(all[3].number(), Some(220.0));
    assert!(all[4].number().unwrap().abs() < 1e-12, "{:?}", all[4]);
}

#[test]
fn long_list_arithmetic_is_compiled_exactly() {
    // Arithmetic on long lists takes a compiled path; a comprehension goes
    // the general way. The two must agree to the bit, and anything the
    // compiled path cannot do (whole-list functions, indexing, complex
    // numbers) must still give the general answer.
    let last = |latex: &[&str]| rows(latex).pop().unwrap();
    let base = [
        "a=1.3",
        "n=\\left[1...300\\right]",
        "m=\\left[1...200\\right]",
        "f\\left(k\\right)=k^2-\\sum_{j=1}^{3}j",
    ];
    let with = |e: &str| {
        let mut all: Vec<&str> = base.to_vec();
        all.push(e);
        last(&all)
    };
    let same = |fast: &str, slow: &str| {
        let (a, b) = (with(fast), with(slow));
        assert!(a.error.is_none() && b.error.is_none(), "{a:?} {b:?}");
        assert_eq!(a.value, b.value, "{fast}");
        assert_eq!(a.points, b.points, "{fast}");
    };
    same(
        "\\left(\\frac{n}{500}\\cos\\left(an\\right),\\frac{n}{500}\\sin\\left(an\\right)\\right)",
        "\\left[\\left(\\frac{k}{500}\\cos\\left(ak\\right),\\frac{k}{500}\\sin\\left(ak\\right)\\right)\\operatorname{for}k=n\\right]",
    );
    same(
        "f\\left(n\\right)+\\sqrt{n}",
        "\\left[f\\left(k\\right)+\\sqrt{k}\\operatorname{for}k=n\\right]",
    );
    same(
        "n+m",
        "\\left[k+m\\left[k\\right]\\operatorname{for}k=\\left[1...200\\right]\\right]",
    );
    same(
        "n-\\operatorname{total}\\left(n\\right)",
        "\\left[k-45150\\operatorname{for}k=n\\right]",
    );
    same(
        "n\\cdot n\\left[2\\right]",
        "\\left[2k\\operatorname{for}k=n\\right]",
    );
    let complex = with("n\\cdot i\\cdot i");
    assert!(complex.error.is_none(), "{complex:?}");
    assert!(complex.points.is_empty());
}

fn rows_3d(latex: &[&str]) -> Vec<RowAnalysis> {
    let input: Vec<RowInput> = latex
        .iter()
        .enumerate()
        .map(|(i, l)| RowInput {
            id: i as u64,
            latex: l.to_string(),
            ..Default::default()
        })
        .collect();
    graph_expr::analyze_mode(&input, &mut Cache::default(), graph_expr::Mode::ThreeD)
}

#[test]
fn three_d_surfaces() {
    let r = rows_3d(&[
        "z=x^2-y^2",
        "\\sin\\left(x\\right)\\cos\\left(y\\right)=z",
        "a=2",
        "z=ar",
        "x^2+y^2",
        "z=\\left[1,2\\right]",
        "1+1",
        "z=b",
        "y=x^2",
        "\\left(1,2\\right)",
        "x+z",
    ]);
    let at = |i: usize, x: f64, y: f64| r[i].surfaces[0].program.eval2(x, y);
    assert!(close(at(0, 3.0, 1.0), 8.0));
    assert!(close(at(1, 1.0, 0.0), 1f64.sin()));
    assert_eq!(r[2].slider, Some(2.0), "variables still work");
    assert!(
        close(at(3, 3.0, 4.0), 10.0),
        "r is the distance from the z axis"
    );
    assert!(close(at(4, 1.0, 2.0), 5.0), "a bare expression is z = …");
    assert_eq!(r[5].surfaces.len(), 2, "one surface per list element");
    assert_eq!(r[6].value, Some(Value::Number(2.0)));
    assert_eq!(r[7].missing, vec!["b".to_string()]);
    assert_eq!(
        r[9].points3,
        vec![[1.0, 2.0, 0.0]],
        "a flat point sits on z = 0"
    );
    assert_eq!(r[8].implicits3.len(), 1, "y = x² is a wall in 3D");
    assert!(r[10].error.is_some() && !r[10].graphed(), "{:?}", r[10]);
    // In 2D the same z = … defines a variable.
    assert_eq!(rows(&["z=3"])[0].kind, RowKind::Variable("z".into()));
}

#[test]
fn three_d_points_and_curves() {
    let r = rows_3d(&[
        "\\left(1,2,3\\right)",
        "p=\\left(1,-2,a\\right)",
        "a=4",
        "p.z+p.x",
        "\\left[\\left(1,0,0\\right),\\left(0,1,0\\right)\\right]+\\left(0,0,1\\right)",
        "\\left(\\cos t,\\sin t,t\\right)",
        "\\left(\\cos t,\\sin t\\right)",
        "2p",
    ]);
    assert_eq!(r[0].points3, vec![[1.0, 2.0, 3.0]]);
    assert_eq!(r[0].value, None, "points are drawn, not echoed");
    assert_eq!(r[1].points3, vec![[1.0, -2.0, 4.0]]);
    assert_eq!(r[3].value, Some(Value::Number(5.0)));
    assert_eq!(r[4].points3, vec![[1.0, 0.0, 1.0], [0.0, 1.0, 1.0]]);
    let c = &r[5].curves3[0];
    assert!(close(c.z.eval(2.0), 2.0) && close(c.x.eval(0.0), 1.0));
    assert_eq!(r[5].parameter.as_ref().unwrap().name, "t");
    assert_eq!(
        r[6].curves3[0].z.eval(1.0),
        0.0,
        "a flat curve sits on z = 0"
    );
    assert_eq!(r[7].points3, vec![[2.0, -4.0, 8.0]]);

    // A flat graph can't draw them.
    let flat = rows(&["\\left(1,2,3\\right)", "\\left(\\cos t,\\sin t,t\\right)"]);
    for row in &flat {
        assert!(row.error.is_some() && !row.graphed(), "{row:?}");
    }
}

#[test]
fn three_d_equations_and_uv_surfaces() {
    let r = rows_3d(&[
        "x^2+y^2+z^2=25",
        "y=x^2",
        "\\rho=5",
        "r=3",
        "\\phi=1",
        "\\left(u,v,uv\\right)",
        "x^2+y^2+z^2<25",
    ]);
    let at = |i: usize, p: [f64; 3]| r[i].implicits3[0].program.eval3(p[0], p[1], p[2]);
    assert!(close(at(0, [3.0, 4.0, 0.0]), 0.0));
    assert!(close(at(0, [0.0, 0.0, 0.0]), -25.0));
    assert!(
        close(at(1, [2.0, 4.0, 7.0]), 0.0),
        "a wall straight up through y = x²"
    );
    assert!(
        close(at(2, [0.0, 3.0, 4.0]), 0.0),
        "ρ is the distance from the origin"
    );
    assert!(
        close(at(3, [3.0, 0.0, 9.0]), 0.0),
        "r is the distance from the z axis"
    );
    assert!(
        close(at(4, [1f64.sin(), 0.0, 1f64.cos()]), 0.0),
        "φ is measured from +z"
    );
    let s = &r[5].param_surfaces[0];
    assert!(close(s.z.eval2(0.5, 0.25), 0.125));
    assert_eq!((s.u, s.v), ((0.0, 1.0), (0.0, 1.0)));
    let solid = &r[6].implicits3[0];
    assert_eq!(solid.shade, Some(Compare::Lt), "an inequality is a solid");
    assert!(close(solid.program.eval3(0.0, 0.0, 0.0), -25.0));
    let r = rows_3d(&["z>x", "-1\\le z<1+x", "x^2>4"]);
    // Inside is where the program is negative: x - z for z > x.
    assert_eq!(r[0].implicits3[0].shade, Some(Compare::Lt));
    assert!(close(r[0].implicits3[0].program.eval3(1.0, 0.0, 3.0), -2.0));
    // A chain is inside where its largest gap is negative.
    let slab = &r[1].implicits3[0];
    assert_eq!(slab.shade, Some(Compare::Le));
    assert!(slab.program.eval3(5.0, 5.0, 0.0) < 0.0);
    assert!(slab.program.eval3(5.0, 5.0, 7.0) > 0.0);
    assert!(r[2].graphed() && r[2].error.is_none(), "{:?}", r[2]);
    // A row that defines u fixes it, and the surface is traced by v alone.
    let r = rows_3d(&["u=2", "\\left(u,v,1\\right)"]);
    assert_eq!(r[0].slider, Some(2.0));
    let s = &r[1].param_surfaces[0];
    assert!(close(s.x.eval2(0.3, 0.6), 2.0));
    assert!(close(s.y.eval2(0.3, 0.6), 0.6));
    // Restrictions bound a solid, and constant bounds on x, y and z make
    // exact boxes, one per list element (a 3D Riemann sum).
    let r = rows_3d(&[
        "w=\\left[0,1\\right]",
        "0\\le z\\le w+1\\left\\{w\\le x\\le w+1\\right\\}\\left\\{0\\le y\\le 2\\right\\}",
        "x^2+y^2+z^2<25\\left\\{x>0\\right\\}",
        "f\\left(x,y\\right)=x^2+y^2",
        "\\operatorname{polygon}\\left(\\left(0,0,0\\right),\\left(1,0,0\\right),\\left(0,1,2\\right)\\right)",
        "z<3",
    ]);
    assert_eq!(
        r[1].boxes3,
        vec![
            [[0.0, 1.0], [0.0, 2.0], [0.0, 1.0]],
            [[1.0, 2.0], [0.0, 2.0], [0.0, 2.0]]
        ]
    );
    let half_ball = &r[2].implicits3[0].program;
    assert!(half_ball.eval3(1.0, 0.0, 0.0) < 0.0);
    assert!(
        half_ball.eval3(-1.0, 0.0, 0.0) > 0.0,
        "the restriction cuts it"
    );
    assert!(
        close(r[3].surfaces[0].program.eval2(1.0, 2.0), 5.0),
        "f(x, y) is a surface"
    );
    assert_eq!(
        r[4].polygons3,
        vec![vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 2.0]]]
    );
    assert_eq!(
        r[5].boxes3,
        vec![[
            [f64::NEG_INFINITY, f64::INFINITY],
            [f64::NEG_INFINITY, f64::INFINITY],
            [f64::NEG_INFINITY, 3.0]
        ]]
    );
    // In 2D, ρ and φ stay ordinary names.
    assert_eq!(rows(&["\\rho=5"])[0].kind, RowKind::Variable("rho".into()));
}

/// The real and imaginary parts of a complex plot at z = x + iy.
fn complex_at(latex: &[&str], x: f64, y: f64) -> (f64, f64) {
    let all = rows(latex);
    let last = all.last().unwrap();
    let plot = last
        .complex
        .first()
        .unwrap_or_else(|| panic!("no complex plot: {last:?}"));
    (plot.re.eval2(x, y), plot.im.eval2(x, y))
}

#[test]
fn complex_plots() {
    let at = |latex: &[&str], x: f64, y: f64| complex_at(latex, x, y);
    let near = |(a, b): (f64, f64), (c, d): (f64, f64)| {
        assert!(close(a, c) && close(b, d), "({a}, {b}) != ({c}, {d})")
    };
    // (1 + 2i)^2 = -3 + 4i
    near(at(&["z^2"], 1.0, 2.0), (-3.0, 4.0));
    near(at(&["\\frac{1}{z}"], 0.0, 2.0), (0.0, -0.5));
    // sin(1 + i) = sin 1 cosh 1 + i cos 1 sinh 1
    let (s, c) = (1f64.sin(), 1f64.cos());
    near(
        at(&["\\sin z"], 1.0, 1.0),
        (s * 1f64.cosh(), c * 1f64.sinh()),
    );
    near(
        at(&["\\cos z"], 1.0, 1.0),
        (c * 1f64.cosh(), -s * 1f64.sinh()),
    );
    near(
        at(&["\\ln z"], 0.0, 1.0),
        (0.0, std::f64::consts::FRAC_PI_2),
    );
    near(at(&["e^{z}"], 0.0, std::f64::consts::PI), (-1.0, 0.0));
    // Functions and sliders work as anywhere else.
    near(
        at(&["f\\left(t\\right)=t^2+1", "f\\left(z\\right)"], 0.0, 1.0),
        (0.0, 0.0),
    );
    near(at(&["a=2", "az"], 1.0, 1.0), (2.0, 2.0));
    // An undefined name is offered as a slider, as on any other row.
    let slider = one("z^2+c");
    assert_eq!(slider.missing, ["c"]);
    // A list draws one plot per element.
    assert_eq!(one("\\left[1,2\\right]z").complex.len(), 2);
    // Real-valued functions of z are plots too.
    near(at(&["\\left|z\\right|"], 3.0, 4.0), (5.0, 0.0));
    assert!(one("z+x").error.is_some());
    // A row defining z makes it an ordinary variable again.
    let defined = rows(&["z=3", "z^2"]);
    assert!(defined[1].complex.is_empty());
    assert_eq!(defined[1].number(), Some(9.0));
    // Bound by a comprehension, z is not free.
    let list = one("\\left[z^2\\ \\operatorname{for}\\ z=\\left[1,2\\right]\\right]");
    assert!(list.complex.is_empty() && list.error.is_none(), "{list:?}");
    // Complex constants take the same functions.
    let sin_i = one("\\sin i");
    assert_eq!(sin_i.value, Some(Value::Complex(0.0, 1f64.sinh())));
}

fn ode(latex: &[&str], seeds: &str) -> RowAnalysis {
    ode_with(latex, seeds, &[])
}

fn ode_with(latex: &[&str], seeds: &str, derivs: &[&str]) -> RowAnalysis {
    let mut input: Vec<RowInput> = latex
        .iter()
        .enumerate()
        .map(|(i, l)| RowInput {
            id: i as u64,
            latex: l.to_string(),
            ..Default::default()
        })
        .collect();
    input.last_mut().unwrap().seeds = Some(seeds.into());
    input.last_mut().unwrap().derivs = derivs.iter().map(|d| d.to_string()).collect();
    analyze(&input).pop().unwrap()
}

#[test]
fn differential_equations_draw_slope_fields_through_initial_points() {
    let r = ode(&["k=2", "y'=k x-y"], "\\left(0,1\\right)");
    assert_eq!(r.kind, RowKind::Curve);
    assert_eq!(r.error, None);
    assert!(r.graphed());
    assert_eq!(r.ode, Some(1));
    let Ode::Explicit { order: 1, f } = &r.odes[0] else {
        panic!("{:?}", r.odes)
    };
    assert!(close(f.eval_n(3.0, &[1.0]), 5.0));
    assert_eq!(r.seeds.len(), 1);
    assert_eq!(r.seeds[0].at, [0.0, 1.0]);
    assert!(
        r.seeds_fixed,
        "plain numbers can be dragged by rewriting them"
    );

    // dy/dx works too, and a list of initial points with sliders in them
    // drags those sliders.
    let r = ode(
        &["a=1", "b=-2", "\\frac{dy}{dx}=\\sin\\left(xy\\right)"],
        "\\left[\\left(a,b\\right),\\left(2,3\\right)\\right]",
    );
    assert_eq!(r.error, None);
    assert_eq!(r.seeds.len(), 2);
    assert_eq!(r.seeds[0].at, [1.0, -2.0]);
    assert_eq!(
        r.seeds[0].drag,
        Some([Some("a".to_string()), Some("b".to_string())])
    );
    assert_eq!(r.seeds[1].drag, None);
    assert!(!r.seeds_fixed);

    // No initial points: just the field. Bad ones say so without hiding it.
    let r = ode(&["y'=y"], "");
    assert_eq!((r.odes.len(), r.seeds.len()), (1, 0));
    let r = ode(&["y'=y"], "3");
    assert_eq!(r.odes.len(), 1);
    assert!(r.seeds_error.is_some());
    let r = ode(&["y'=y"], "\\left(c,1\\right)");
    assert_eq!(r.missing, vec!["c".to_string()], "offers c as a slider");

    // Undefined names in the slope are offered as sliders too.
    let r = one("y'=mx");
    assert_eq!(r.missing, vec!["m".to_string()]);
    assert!(r.odes.is_empty());
    assert!(one("y'=\\left(1,2\\right)").error.is_some());
}

#[test]
fn higher_order_implicit_and_system_equations() {
    // y'' = -k y' - y with y'(x₀) for each start from its own box.
    let r = ode_with(
        &["k=0.5", "y''=-ky'-y"],
        "\\left[\\left(0,1\\right),\\left(1,2\\right)\\right]",
        &["\\left[3,4\\right]"],
    );
    assert_eq!(r.error, None);
    assert_eq!(r.ode, Some(2));
    let Ode::Explicit { order: 2, f } = &r.odes[0] else {
        panic!("{:?}", r.odes)
    };
    // At y = 1, y' = 2: -0.5·2 - 1.
    assert!(close(f.eval_n(0.0, &[1.0, 2.0]), -2.0));
    assert_eq!(r.seeds[0].derivs, vec![3.0]);
    assert_eq!(r.seeds[1].derivs, vec![4.0]);
    // Left out, the initial slope is 0; one number serves every start.
    let r = ode_with(&["\\frac{d^3y}{dx^3}=y"], "\\left(0,1\\right)", &["", "2"]);
    assert_eq!(r.ode, Some(3));
    assert_eq!(r.seeds[0].derivs, vec![0.0, 2.0]);
    let r = ode_with(&["y''=y"], "\\left(0,1\\right)", &["\\left(1,2\\right)"]);
    assert!(r.derivs_error.is_some());

    // Not solved for y': drawn by solving F(x, y, y') = 0.
    for latex in ["\\left(y'\\right)^2+y^2=1", "xy'+y=0", "y'=y'^3+x"] {
        let r = ode(&[latex], "\\left(0,0\\right)");
        assert_eq!(r.error, None, "{latex}");
        assert!(
            matches!(r.odes[0], Ode::Implicit { order: 1, .. }),
            "{latex}"
        );
    }
    let r = one("y''+y=0");
    assert!(
        matches!(&r.odes[0], Ode::Implicit { order: 2, f } if close(f.eval_n(0.0, &[1.0, 5.0, -1.0]), 0.0))
    );

    // Systems, in t, x and y.
    for latex in [
        "x'=y,y'=-x",
        "\\left(x',y'\\right)=\\left(y,-x\\right)",
        "\\frac{dx}{dt}=y,\\frac{dy}{dt}=-x",
    ] {
        let r = one(latex);
        assert_eq!(r.error, None, "{latex}");
        let Ode::System { f, g } = &r.odes[0] else {
            panic!("{latex}: {:?}", r.odes)
        };
        assert!(close(f.eval3(1.0, 2.0, 0.0), 2.0) && close(g.eval3(1.0, 2.0, 0.0), -1.0));
        assert!(!f.uses_z(), "autonomous");
        assert_eq!(r.ode, Some(1));
    }
    let r = one("x'=t,y'=1");
    let Ode::System { f, .. } = &r.odes[0] else {
        panic!()
    };
    assert!(f.uses_z(), "reads t");

    // Mistakes.
    assert!(one("x'=y").error.is_some());
    assert!(one("y'>x").error.is_some());
    assert!(one("y'''''''=y").error.is_some(), "order 7 is too high");
    assert!(one("x'=y',y'=x").error.is_some());
    let r = rows(&["a=y'"]).remove(0);
    assert!(r.missing.is_empty(), "y' is never offered as a slider");
}
