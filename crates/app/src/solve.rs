//! The Solve button: runs symbolic algebra on one row when asked, never
//! while typing, and sends the working back to the panel.

use graph_expr::{cas, symbolic};
use serde_json::{json, Value};

/// The panel's reply for row `id`, whose LaTeX is `latex`. The other rows
/// supply functions the row may use.
pub fn reply(rows: &Value, id: u64, latex: &str, op: Option<&str>) -> Value {
    let others: Vec<String> = rows
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["id"].as_u64() != Some(id))
        .filter_map(|r| r["latex"].as_str())
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect();
    // A differential equation's initial point and derivatives.
    let row = rows
        .as_array()
        .into_iter()
        .flatten()
        .find(|r| r["id"].as_u64() == Some(id));
    let initial = symbolic::Initial {
        seeds: row.and_then(|r| r["seeds"].as_str()).map(str::to_owned),
        derivs: row
            .and_then(|r| r["derivs"].as_array())
            .into_iter()
            .flatten()
            .map(|d| d.as_str().unwrap_or("").to_owned())
            .collect(),
    };
    let report = solve(latex, &others, op, &initial, crate::giac::available());
    json!({
        "id": id,
        "op": report.op,
        "choices": report.choices.iter().map(|c| json!({"op": c.op, "label": c.label})).collect::<Vec<_>>(),
        "steps": report.steps.iter().map(|s| json!({"note": s.note, "latex": s.latex})).collect::<Vec<_>>(),
        "result": report.result,
        "answer": report.answer,
        "error": report.error,
    })
}

/// Rust's own algebra first, then Giac (when `giac` is there) for what it
/// can't do or when one of Giac's choices is picked.
fn solve(
    latex: &str,
    others: &[String],
    op: Option<&str>,
    initial: &symbolic::Initial,
    giac: bool,
) -> symbolic::Report {
    if let Some(op) = op.filter(|o| o.starts_with("giac:")) {
        return with_giac(latex, others, Some(op), giac);
    }
    let ours = {
        let (latex, others, op, initial) = (
            latex.to_owned(),
            others.to_vec(),
            op.map(str::to_owned),
            initial.clone(),
        );
        // A bug in the algebra must not take the window down with it.
        std::panic::catch_unwind(move || {
            symbolic::run_row(&latex, &others, op.as_deref(), &initial)
        })
        .unwrap_or_else(|_| symbolic::Report {
            error: Some("Something went wrong while solving this".into()),
            ..Default::default()
        })
    };
    if !giac {
        return ours;
    }
    if ours.error.is_some() && op.is_none() {
        let theirs = with_giac(latex, others, None, giac);
        if theirs.error.is_none() {
            return merge_choices(theirs, &ours.choices);
        }
        return ours;
    }
    // Giac's other ways of working on the row, after Solve's own.
    match cas::plan(latex, others, None) {
        Ok(plan) => merge_choices(ours, &plan.choices),
        Err(_) => ours,
    }
}

fn with_giac(latex: &str, others: &[String], op: Option<&str>, giac: bool) -> symbolic::Report {
    if !giac {
        return symbolic::Report {
            error: Some("Giac isn't part of this build".into()),
            ..Default::default()
        };
    }
    let plan = match cas::plan(latex, others, op) {
        Ok(plan) => plan,
        Err(e) => {
            return symbolic::Report {
                error: Some(e),
                ..Default::default()
            }
        }
    };
    let answer = crate::giac::run(&plan);
    std::panic::catch_unwind(|| cas::finish(&plan, answer)).unwrap_or_else(|_| symbolic::Report {
        error: Some("Something went wrong while reading Giac's answer".into()),
        ..Default::default()
    })
}

fn merge_choices(mut report: symbolic::Report, more: &[symbolic::Choice]) -> symbolic::Report {
    for c in more {
        if !report
            .choices
            .iter()
            .any(|d| d.op == c.op || d.label == c.label)
        {
            report.choices.push(c.clone());
        }
    }
    report
}

/// A document's live lines as rows for `reply`, numbered from 1 so the
/// line being solved can be row 0.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn doc_rows(lines: &Value) -> Value {
    Value::Array(
        lines
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .enumerate()
            .map(|(i, latex)| json!({ "id": i + 1, "latex": latex }))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_with_steps() {
        let rows = json!([{"id": 1, "latex": "f(x)=x^2-1"}, {"id": 2, "latex": "f(x)=3"}]);
        let r = reply(&rows, 2, "f(x)=3", None);
        assert_eq!(r["result"], "x=-2\\quad\\text{or}\\quad x=2");
        assert!(r["steps"].as_array().unwrap().len() > 1);
        assert_eq!(r["error"], Value::Null);
    }

    /// Runs with the real Giac worker when `GRAPHCALC_GIAC` names it (the
    /// Windows CI build does); skipped otherwise.
    #[test]
    fn giac_answers_are_checked() {
        if !crate::giac::available() {
            eprintln!("GRAPHCALC_GIAC isn't set; skipping");
            return;
        }
        let none = symbolic::Initial::default();
        let cases: &[(&str, &[&str], Option<&str>, &str)] = &[
            (
                "\\int_{ }^{ }e^{-x^{2}}dx",
                &[],
                Some("giac:integrate"),
                "erf",
            ),
            (
                "\\int_{0}^{\\infty}e^{-x^{2}}dx",
                &[],
                Some("giac:integrate"),
                "\\sqrt{\\pi}",
            ),
            ("\\lim_{x\\to0}\\frac{\\sin x}{x}", &[], None, "1"),
            (
                "\\sum_{n=1}^{\\infty}\\frac{1}{n^{2}}",
                &[],
                Some("giac:simplify"),
                "\\pi",
            ),
            ("x^{4}-1", &[], Some("giac:factor"), "x^{2}+1"),
            ("t^{2}", &[], Some("giac:laplace"), "s"),
            (
                "\\int_{0}^{a}f\\left(x\\right)dx",
                &["a=2", "f\\left(t\\right)=t^{2}"],
                Some("giac:integrate"),
                "8",
            ),
        ];
        for (latex, others, op, want) in cases {
            let others: Vec<String> = others.iter().map(|s| s.to_string()).collect();
            let r = solve(latex, &others, *op, &none, true);
            let result = r.result.clone().unwrap_or_default();
            eprintln!(
                "{latex} -> {result:?} {:?}",
                r.steps.last().map(|s| &s.note)
            );
            assert!(r.error.is_none(), "{latex}: {r:?}");
            assert!(result.contains(want), "{latex}: {result}");
            let last = &r.steps.last().unwrap().note;
            assert!(!last.contains("doesn't agree"), "{latex}: {r:?}");
        }
        // Something Rust can't do falls through to Giac by itself.
        let r = solve("\\lim_{x\\to0^{+}}x\\ln x", &[], None, &none, true);
        assert!(r.error.is_none(), "{r:?}");
        // Stopping a problem leaves Giac ready for the next.
        let started = std::thread::spawn(|| {
            let none = symbolic::Initial::default();
            solve(
                "\\sum_{n=1}^{100000000}\\frac{1}{n}",
                &[],
                Some("giac:simplify"),
                &none,
                true,
            )
        });
        std::thread::sleep(std::time::Duration::from_millis(500));
        crate::giac::cancel();
        let _ = started.join();
        let r = solve("x^{2}-1", &[], Some("giac:factor"), &none, true);
        assert!(r.error.is_none(), "{r:?}");
    }
}
