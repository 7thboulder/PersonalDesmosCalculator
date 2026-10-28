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
