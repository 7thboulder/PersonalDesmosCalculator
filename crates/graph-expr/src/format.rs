//! Number formatting in Desmos's style, as LaTeX for the panel to render.

/// Formats a computed value the way Desmos shows results: up to 12
/// significant digits, scientific notation for very large or small numbers.
pub fn value_latex(v: f64) -> String {
    if v.is_nan() {
        return "\\operatorname{undefined}".into();
    }
    if v.is_infinite() {
        return if v > 0.0 {
            "\\infty".into()
        } else {
            "-\\infty".into()
        };
    }
    if v == 0.0 {
        return "0".into();
    }
    let a = v.abs();
    if !(1e-6..1e12).contains(&a) {
        let exp = a.log10().floor() as i32;
        let mut mantissa = v / 10f64.powi(exp);
        // Rounding can carry into the next power of ten (9.9999 -> 10).
        let mut exp = exp;
        if trim(format!("{:.9}", mantissa.abs())).starts_with("10") {
            mantissa /= 10.0;
            exp += 1;
        }
        let m = trim(format!("{mantissa:.9}"));
        return format!("{m}\\times10^{{{exp}}}");
    }
    let digits = (11 - a.log10().floor() as i32).clamp(0, 17) as usize;
    trim(format!("{v:.digits$}"))
}

/// Most list elements shown under a row before the rest are elided.
const SHOWN_ELEMENTS: usize = 12;

/// Any computed result as LaTeX: a number, a point, or a list of either.
pub fn result_latex(v: &crate::Value) -> String {
    use crate::Value;
    let point = |p: [f64; 2]| format!("\\left({},{}\\right)", value_latex(p[0]), value_latex(p[1]));
    let list = |items: Vec<String>| {
        let n = items.len();
        let mut shown: Vec<String> = items.into_iter().take(SHOWN_ELEMENTS).collect();
        if n > SHOWN_ELEMENTS {
            shown.push("...".into());
        }
        format!("\\left[{}\\right]", shown.join(","))
    };
    match v {
        Value::Number(n) => value_latex(*n),
        Value::Point(a, b) => point([*a, *b]),
        Value::Numbers(ns) => list(ns.iter().map(|n| value_latex(*n)).collect()),
        Value::Points(ps) => list(ps.iter().map(|p| point(*p)).collect()),
    }
}

/// Drops trailing zeros and a trailing decimal point.
fn trim(s: String) -> String {
    if !s.contains('.') {
        return s;
    }
    let t = s.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" {
        "0".into()
    } else {
        t.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desmos_style_values() {
        assert_eq!(value_latex(5.0), "5");
        assert_eq!(value_latex(-2.5), "-2.5");
        assert_eq!(value_latex(1.0 / 3.0), "0.333333333333");
        assert_eq!(value_latex(std::f64::consts::PI), "3.14159265359");
        assert_eq!(value_latex(0.1 + 0.2), "0.3");
        assert_eq!(value_latex(1234567.0), "1234567");
        assert_eq!(value_latex(6.02e23), "6.02\\times10^{23}");
        assert_eq!(value_latex(1.5e-9), "1.5\\times10^{-9}");
        assert_eq!(value_latex(f64::NAN), "\\operatorname{undefined}");
    }

    #[test]
    fn lists_and_points() {
        use crate::Value;
        assert_eq!(
            result_latex(&Value::Point(1.0, -2.5)),
            "\\left(1,-2.5\\right)"
        );
        assert_eq!(
            result_latex(&Value::Numbers(vec![1.0, 4.0, 9.0])),
            "\\left[1,4,9\\right]"
        );
        let long = result_latex(&Value::Numbers((1..=20).map(f64::from).collect()));
        assert!(long.ends_with(",12,...\\right]"), "{long}");
    }
}
