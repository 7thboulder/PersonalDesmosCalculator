use super::*;

fn solved(latex: &str, op: Option<&str>) -> Report {
    let r = run(latex, &[], op);
    assert!(r.error.is_none(), "{latex}: {:?}\n{:#?}", r.error, r.steps);
    r
}

fn result(latex: &str, op: Option<&str>) -> String {
    solved(latex, op).result.unwrap()
}

#[test]
fn linear_and_quadratic() {
    assert_eq!(result("2x+3=7", None), "x=2");
    assert_eq!(result("x^2-5x+6=0", None), "x=2\\quad\\text{or}\\quad x=3");
    let r = result("x^2-2x-1=0", None);
    assert!(r.contains("\\sqrt{2}"), "{r}");
}

#[test]
fn checks_extraneous_roots() {
    let r = solved("\\sqrt{x+3}=x-3", None);
    assert_eq!(r.result.unwrap(), "x=6");
    assert!(r.steps.iter().any(|s| s.note.contains("drop it")));
}

#[test]
fn trig_and_exponentials() {
    let r = result("2\\sin x=1", None);
    assert!(r.contains("2\\pi k"), "{r}");
    assert_eq!(
        result("e^{x}=5", None),
        "x=\\ln\\left(5\\right)\\approx 1.60943791243"
    );
    let r = result("e^{2x}-3e^{x}+2=0", None);
    assert!(r.contains("\\ln\\left(2\\right)"), "{r}");
}

#[test]
fn fractions_and_cubics() {
    assert_eq!(result("\\frac{1}{x}+\\frac{1}{2}=1", None), "x=2");
    assert_eq!(
        result("x^3-6x^2+11x-6=0", None),
        "x=1\\quad\\text{or}\\quad x=2\\quad\\text{or}\\quad x=3"
    );
}

#[test]
fn inequalities() {
    let r = solved("x^2<4", None);
    assert_eq!(r.result.unwrap(), "-2<x<2");
    assert_eq!(r.answer.unwrap(), "-2<x<2");
    assert_eq!(result("2x+1\\ge 5", None), "x\\ge 2");
}

#[test]
fn rows_offer_the_right_operations() {
    let r = solved("y=x^2-4", None);
    assert_eq!(r.op, "roots:x");
    assert_eq!(
        r.answer.unwrap(),
        "\\left[\\left(-2,0\\right),\\left(2,0\\right)\\right]"
    );
    assert_eq!(
        result("\\frac{1}{2}+\\frac{1}{3}", None),
        "\\frac{5}{6}\\approx 0.833333333333"
    );
    assert_eq!(
        result("\\sqrt{12}", None),
        "2\\sqrt{3}\\approx 3.46410161514"
    );
}

#[test]
fn derivatives_and_integrals() {
    assert_eq!(
        result("\\frac{d}{dx}x^2\\sin x", None),
        "x^{2}\\cos\\left(x\\right)+2x\\sin\\left(x\\right)"
    );
    assert_eq!(result("\\int x^{2}dx", None), "\\frac{x^{3}}{3}+C");
    assert_eq!(result("\\int_{ }^{ }x^2dx", None), "\\frac{x^{3}}{3}+C");
    assert_eq!(
        result("\\int_{0}^{1}x^{2}dx", None),
        "\\frac{1}{3}\\approx 0.333333333333"
    );
    assert_eq!(result("\\int_{0}^{\\pi}\\sin x dx", None), "2");
}

#[test]
fn factoring() {
    assert_eq!(
        result("2x^2-8", Some("factor")),
        "2\\left(x-2\\right)\\left(x+2\\right)"
    );
    assert_eq!(result("x^2+2x+1", Some("factor")), "\\left(x+1\\right)^{2}");
}

#[test]
fn uses_functions_from_other_rows() {
    let r = run("f(x)=0", &["f(x)=x^2-9".into()], None);
    assert_eq!(r.result.unwrap(), "x=-3\\quad\\text{or}\\quad x=3");
}

#[test]
fn symbolic_letters() {
    let r = result("ax+b=0", Some("solve:x"));
    assert_eq!(r, "x=-\\frac{b}{a}");
}

fn ode(latex: &str, seeds: Option<&str>, derivs: &[&str]) -> String {
    let initial = Initial {
        seeds: seeds.map(str::to_owned),
        derivs: derivs.iter().map(|d| d.to_string()).collect(),
    };
    let op = if seeds.is_some() {
        "ode"
    } else {
        "ode:general"
    };
    let r = run_row(latex, &[], Some(op), &initial);
    assert!(r.error.is_none(), "{latex}: {:?}\n{:#?}", r.error, r.steps);
    r.result.unwrap()
}

#[test]
fn first_order_equations() {
    // Linear, by an integrating factor.
    assert_eq!(ode("y'=x-y", None, &[]), "y=\\frac{C}{e^{x}}+x-1");
    assert_eq!(ode("xy'+y=x^2", None, &[]), "y=\\frac{x^{3}+C}{3x}");
    // Separable, solved for y with the constants folded together.
    assert_eq!(ode("y'=y^2", None, &[]), "y=-\\frac{1}{C+x}");
    assert_eq!(ode("y'=ky", None, &[]), "y=Ce^{kx}");
    assert_eq!(ode("y'=1+y^2", None, &[]), "y=\\tan\\left(C+x\\right)");
    assert_eq!(
        ode("y'=y\\left(1-y\\right)", None, &[]),
        "y=\\frac{Ce^{x}}{Ce^{x}+1}"
    );
    // Exact.
    assert_eq!(
        ode("2xy+\\left(x^2+3y^2\\right)y'=0", None, &[]),
        "x^{2}y+y^{3}=C"
    );
    // Bernoulli and homogeneous.
    assert!(ode("y'+y=xy^2", None, &[]).starts_with("y="));
    assert!(ode("y'=\\frac{y}{x}+\\frac{x}{y}", None, &[]).contains("\\ln"));
}

#[test]
fn initial_conditions() {
    assert_eq!(
        ode("y'=x-y", Some("\\left(0,1\\right)"), &[]),
        "y=x+\\frac{2}{e^{x}}-1"
    );
    assert_eq!(
        ode("y''+y=0", Some("\\left(0,1\\right)"), &["2"]),
        "y=\\cos\\left(x\\right)+2\\sin\\left(x\\right)"
    );
    assert_eq!(
        ode("y'=\\frac{x}{y}", Some("\\left(0,2\\right)"), &[]),
        "y=\\sqrt{x^{2}+4}"
    );
}

#[test]
fn higher_order_equations() {
    assert_eq!(
        ode("y''+\\omega^2y=0", None, &[]),
        "y=C_{1}\\cos\\left(\\omega x\\right)+C_{2}\\sin\\left(\\omega x\\right)"
    );
    assert_eq!(
        ode("y''+y=\\cos x", None, &[]),
        "y=C_{1}\\cos\\left(x\\right)+C_{2}\\sin\\left(x\\right)+\\frac{x\\sin\\left(x\\right)}{2}"
    );
    assert_eq!(ode("y''-2y'+y=0", None, &[]), "y=C_{2}xe^{x}+C_{1}e^{x}");
    assert_eq!(
        ode("x^2y''+xy'-y=0", None, &[]),
        "y=C_{1}x+\\frac{C_{2}}{x}"
    );
    assert_eq!(ode("y''=\\frac{y'}{x}", None, &[]), "y=C_{1}x^{2}+C_{2}");
    // Variation of parameters.
    assert!(ode("y''+y=\\tan x", None, &[]).contains("\\ln"));
    // Quartic characteristic equations with exact complex roots.
    assert!(ode("y''''+y=0", None, &[]).contains("\\sqrt{2}"));
}

#[test]
fn systems() {
    let r = run_row(
        "x'=y,y'=-x",
        &[],
        Some("ode"),
        &Initial {
            seeds: Some("\\left(1,0\\right)".into()),
            derivs: Vec::new(),
        },
    );
    assert_eq!(
        r.result.unwrap(),
        "\\left(x,y\\right)=\\left(\\cos\\left(t\\right),-\\sin\\left(t\\right)\\right)"
    );
}

#[test]
fn harder_integrals() {
    assert_eq!(
        result("\\int\\sqrt{1-x^2}dx", None),
        "\\frac{x\\sqrt{-x^{2}+1}+\\arcsin\\left(x\\right)}{2}+C"
    );
    assert_eq!(
        result("\\int\\frac{1}{x^2\\sqrt{x^2+4}}dx", None),
        "-\\frac{\\sqrt{x^{2}+4}}{4x}+C"
    );
    assert_eq!(
        result("\\int\\frac{1}{\\left(x^2+1\\right)^2}dx", None),
        "\\frac{\\arctan\\left(x\\right)}{2}+\\frac{x}{2\\left(x^{2}+1\\right)}+C"
    );
    assert!(
        result("\\int\\sec^3x\\ dx", None).contains("\\sec\\left(x\\right)\\tan\\left(x\\right)")
    );
    assert!(
        result("\\int\\frac{1}{2+\\cos x}dx", None).contains("\\tan\\left(\\frac{x}{2}\\right)")
    );
    let r = run("\\int e^{-x^2}dx", &[], None);
    assert!(r.error.unwrap().contains("elementary"));
}

#[test]
fn integrals_without_bounds_graph() {
    let s = crate::parse("y=\\int x\\cos x\\ dx").unwrap();
    let s = resolve_indefinite(s, &[]).unwrap();
    let Statement::Relation(_, _, rhs) = s else {
        panic!()
    };
    assert!(!format!("{rhs:?}").contains("Integral"));
}
