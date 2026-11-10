//! Infinite sums and products, worked out numerically with a check that
//! they converge, and reported as diverging when they don't. Giac is asked
//! for an exact form afterwards (see `cas`), and the two must agree.

use super::Steps;
use crate::ast::{Expr, Repeat, Statement};
use crate::series::Outcome;

/// A sum or product with an infinite bound: whether it is a product, and
/// its variable.
pub struct Series {
    pub product: bool,
    pub var: String,
}

/// The sum or product `source` is, when a bound is infinite.
pub fn read(source: &str) -> Option<Series> {
    let Ok(Statement::Expr(Expr::Repeat(kind, var, lo, hi, _))) = crate::parse(source) else {
        return None;
    };
    let inf = |e: &Expr| super::convert::infinity(e).is_some();
    (inf(&lo) || inf(&hi)).then(|| Series {
        product: kind == Repeat::Product,
        var,
    })
}

/// How many digits `error` leaves trustworthy in `value`, as LaTeX.
pub fn shown(value: f64, error: f64) -> String {
    crate::format::value_latex(crate::series::rounded(value, error))
}

/// What the series does, in words, for a step: None when it converges.
pub fn divergence(o: &Outcome, product: bool, var: &str) -> Option<(String, String)> {
    let v = super::latex::name(var);
    let (what, terms, unit) = if product {
        ("product", "factors", "1")
    } else {
        ("sum", "terms", "0")
    };
    Some(match o {
        Outcome::Converges { .. } => return None,
        Outcome::Infinite(s) => {
            let inf = if *s > 0 { "\\infty" } else { "-\\infty" };
            (
                format!(
                    "The partial {what}s grow without bound as ${v}$ increases, so the {what} diverges to ${inf}$"
                ),
                format!("\\text{{diverges to }}{inf}"),
            )
        }
        Outcome::TermsDontVanish => (
            format!(
                "The {terms} don't approach {unit} as ${v}\\to\\infty$, so the partial {what}s never settle: the {what} diverges"
            ),
            "\\text{diverges}".into(),
        ),
        Outcome::Undefined => (
            format!("One of the {terms} is undefined, so the {what} is too"),
            "\\text{undefined}".into(),
        ),
        Outcome::Unsure => (
            format!(
                "After a million {terms} the partial {what}s are still moving, too slowly to tell whether the {what} converges"
            ),
            String::new(),
        ),
    })
}

/// Solve's own answer: the numeric value with how it was found, or the
/// reason it diverges.
pub fn solve(
    source: &str,
    others: &[String],
    series: &Series,
    st: &mut Steps,
) -> Result<(String, Option<String>), String> {
    let what = if series.product { "product" } else { "sum" };
    let outcome = crate::series::outcome_of(source, others)
        .ok_or_else(|| format!("Solve can only work out an infinite {what} of real numbers"))?;
    if let Some((note, result)) = divergence(&outcome, series.product, &series.var) {
        if result.is_empty() {
            return Err(note);
        }
        st.push(format!("Checked numerically: {}", lower_first(&note)), "");
        return Ok((result, None));
    }
    let Outcome::Converges { value, error } = outcome else {
        unreachable!()
    };
    // A partial sum, to show how far the terms alone get.
    let upto = crate::cas::with_upper(source, 1000).filter(|q| !q.contains("\\infty"));
    if let Some(p) = upto
        .as_deref()
        .and_then(|q| crate::cas::number_of_public(q, others))
        .filter(|p| p.is_finite())
    {
        st.push(
            if series.product {
                "Multiply the first thousand factors".to_owned()
            } else {
                "Add up the first thousand terms".to_owned()
            },
            format!(
                "{}\\approx {}",
                upto.as_deref().unwrap_or_default(),
                crate::format::value_latex(p)
            ),
        );
    }
    let v = shown(value, error);
    st.push(
        format!(
            "Take up to a million terms numerically, and estimate the rest from how the partial {what}s settle: they converge"
        ),
        format!("{source}\\approx {v}"),
    );
    Ok((format!("\\approx {v}"), Some(v)))
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}
