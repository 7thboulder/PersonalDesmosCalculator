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
    // Only the elements shown are written out; a long list can have
    // thousands.
    let list = |n: usize, items: &mut dyn Iterator<Item = String>| {
        let mut shown: Vec<String> = items.take(SHOWN_ELEMENTS).collect();
        if n > SHOWN_ELEMENTS {
            shown.push("...".into());
        }
        format!("\\left[{}\\right]", shown.join(","))
    };
    match v {
        Value::Number(n) => value_latex(*n),
        Value::Point(a, b) => point([*a, *b]),
        Value::Numbers(ns) => list(ns.len(), &mut ns.iter().map(|n| value_latex(*n))),
        Value::Points(ps) => list(ps.len(), &mut ps.iter().map(|p| point(*p))),
        Value::Point3(p) => point3(p, &value_latex),
        Value::Points3(ps) => list(ps.len(), &mut ps.iter().map(|p| point3(p, &value_latex))),
        Value::Polygon(p) => polygon(p, &point),
        Value::Polygons(ps) => list(ps.len(), &mut ps.iter().map(|p| polygon(p, &point))),
        Value::Polygon3(p) => polygon(p, &|q: [f64; 3]| point3(&q, &value_latex)),
        Value::Polygons3(ps) => list(
            ps.len(),
            &mut ps
                .iter()
                .map(|p| polygon(p, &|q: [f64; 3]| point3(&q, &value_latex))),
        ),
        Value::Complex(a, b) => complex(*a, *b, &value_latex),
        Value::Complexes(zs) => list(
            zs.len(),
            &mut zs.iter().map(|z| complex(z[0], z[1], &value_latex)),
        ),
    }
}

/// `(1,2,3)`, with each coordinate written by `number`.
fn point3(p: &[f64; 3], number: &dyn Fn(f64) -> String) -> String {
    format!(
        "\\left({},{},{}\\right)",
        number(p[0]),
        number(p[1]),
        number(p[2])
    )
}

/// `3+2i`, `-i`, `0.5-1.25i`, with each part written by `number`.
fn complex(re: f64, im: f64, number: &dyn Fn(f64) -> String) -> String {
    if im == 0.0 || re.is_nan() || im.is_nan() {
        return number(if im.is_nan() { f64::NAN } else { re });
    }
    let magnitude = match number(im.abs()).as_str() {
        "1" => String::new(),
        m => m.to_owned(),
    };
    let sign = if im < 0.0 { "-" } else { "+" };
    if re == 0.0 {
        let lead = if im < 0.0 { "-" } else { "" };
        return format!("{lead}{magnitude}i");
    }
    format!("{}{sign}{magnitude}i", number(re))
}

fn polygon<P: Copy>(vertices: &[P], point: &dyn Fn(P) -> String) -> String {
    let shown: Vec<String> = vertices
        .iter()
        .take(SHOWN_ELEMENTS)
        .map(|p| point(*p))
        .collect();
    let more = if vertices.len() > SHOWN_ELEMENTS {
        ",..."
    } else {
        ""
    };
    format!(
        "\\operatorname{{polygon}}\\left({}{more}\\right)",
        shown.join(",")
    )
}

/// A number written so that reading it back gives exactly the same value,
/// for an action to write into a row.
pub fn exact_number(v: f64) -> String {
    if v.is_nan() {
        return "\\frac{0}{0}".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "\\infty" } else { "-\\infty" }.into();
    }
    if v == 0.0 {
        return "0".into();
    }
    // Rust prints the shortest digits that read back exactly, without an
    // exponent; very large and very small numbers get one in LaTeX form.
    let plain = format!("{v}");
    if plain.len() <= 24 {
        return plain;
    }
    let sci = format!("{v:e}");
    let (mantissa, exponent) = sci.split_once('e').expect("exponent form");
    format!("{mantissa}\\times10^{{{exponent}}}")
}

/// A value written out in full, so an action can store it in a row.
pub fn exact_latex(v: &crate::Value) -> String {
    use crate::Value;
    let point = |p: [f64; 2]| {
        format!(
            "\\left({},{}\\right)",
            exact_number(p[0]),
            exact_number(p[1])
        )
    };
    let list = |items: Vec<String>| format!("\\left[{}\\right]", items.join(","));
    let polygon = |p: &Vec<[f64; 2]>| {
        let vertices: Vec<String> = p.iter().map(|q| point(*q)).collect();
        format!("\\operatorname{{polygon}}\\left({}\\right)", list(vertices))
    };
    let polygon3 = |p: &Vec<[f64; 3]>| {
        let vertices: Vec<String> = p.iter().map(|q| point3(q, &exact_number)).collect();
        format!("\\operatorname{{polygon}}\\left({}\\right)", list(vertices))
    };
    match v {
        Value::Number(n) => exact_number(*n),
        Value::Point(a, b) => point([*a, *b]),
        Value::Numbers(ns) => list(ns.iter().map(|n| exact_number(*n)).collect()),
        Value::Points(ps) => list(ps.iter().map(|p| point(*p)).collect()),
        Value::Point3(p) => point3(p, &exact_number),
        Value::Points3(ps) => list(ps.iter().map(|p| point3(p, &exact_number)).collect()),
        Value::Polygon(p) => polygon(p),
        Value::Polygons(ps) => list(ps.iter().map(polygon).collect()),
        Value::Polygon3(p) => polygon3(p),
        Value::Polygons3(ps) => list(ps.iter().map(polygon3).collect()),
        Value::Complex(a, b) => complex(*a, *b, &exact_number),
        Value::Complexes(zs) => list(
            zs.iter()
                .map(|z| complex(z[0], z[1], &exact_number))
                .collect(),
        ),
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
