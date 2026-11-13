//! The Solve button: runs symbolic algebra on one row when asked, never
//! while typing, and sends the working back to the panel.

use graph_expr::{cas, symbolic};
use serde_json::{json, Value};

/// The panel's reply for row `id`, whose LaTeX is `latex`. The other rows
/// supply functions the row may use.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn reply(rows: &Value, id: u64, latex: &str, op: Option<&str>) -> Value {
    reply_in(rows, id, latex, op, false)
}

/// As `reply`, with angles in degrees when `degrees` is set, as the graph's
/// degree mode has them.
pub fn reply_in(rows: &Value, id: u64, latex: &str, op: Option<&str>, degrees: bool) -> Value {
    let others: Vec<String> = rows
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["id"].as_u64() != Some(id))
        .filter_map(row_latex)
        .filter(|l| !l.is_empty())
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
        axes: row.and_then(|r| ode_axes(&r["axes"])),
        start: row.map_or_else(Default::default, |r| ode_start(&r["start"])),
    };
    let mut report = solve_in(
        latex,
        &others,
        op,
        &initial,
        crate::giac::available(),
        degrees,
    );
    symbolic::in_typed_letters(&mut report, latex);
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

/// A row as LaTeX: a matrix row's grid becomes `A = [matrix]`, so Solve
/// can work with it.
fn row_latex(r: &Value) -> Option<String> {
    let latex = r["latex"].as_str()?;
    let Some(grid) = r["matrix"].as_array() else {
        return Some(latex.to_owned());
    };
    let rows: Vec<String> = grid
        .iter()
        .map(|cells| {
            cells
                .as_array()
                .map_or(&[][..], Vec::as_slice)
                .iter()
                .map(|c| match c.as_str().unwrap_or("").trim() {
                    "" => "0".to_owned(),
                    t => t.to_owned(),
                })
                .collect::<Vec<_>>()
                .join("&")
        })
        .collect();
    Some(format!(
        "{latex}=\\begin{{bmatrix}}{}\\end{{bmatrix}}",
        rows.join("\\\\")
    ))
}

/// As `solve`, in degree mode when `degrees` is set. Solve's own algebra
/// works in radians, so a problem with trig in it goes to Giac, which
/// works in degrees, and its answer is checked in degrees.
fn solve_in(
    latex: &str,
    others: &[String],
    op: Option<&str>,
    initial: &symbolic::Initial,
    giac: bool,
    degrees: bool,
) -> symbolic::Report {
    let angles = cas::uses_angles(latex) || others.iter().any(|o| cas::uses_angles(o));
    if !degrees || !angles {
        return solve(latex, others, op, initial, giac);
    }
    if !giac {
        return symbolic::Report {
            error: Some(
                "Solving trig in degree mode needs Giac, which isn't part of this build".into(),
            ),
            ..Default::default()
        };
    }
    let op = op.filter(|o| o.starts_with("giac:"));
    let mut report = cas::in_degrees(true, || with_giac(latex, others, op, giac));
    if let Some(first) = report.steps.first_mut() {
        first.note = format!("{} (angles in degrees)", first.note);
    }
    report
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
    let mut ours = ours;
    // Solve's own answer is only numeric: Giac may know an exact one.
    if ours.error.is_none() && op.is_none() && only_numeric(&ours) {
        let theirs = with_giac(latex, others, None, giac);
        if theirs.error.is_none() && exact(&theirs) {
            return merge_choices(theirs, &ours.choices);
        }
    }
    if ours.error.is_none() && ours.op == "simplify" {
        ours = tidier(ours, latex, others);
    }
    // Giac's other ways of working on the row, after Solve's own.
    match cas::plan(latex, others, None) {
        Ok(plan) => merge_choices(ours, &plan.choices),
        Err(_) => ours,
    }
}

/// Whether a step (not the final check) says the answer was found
/// numerically.
fn only_numeric(r: &symbolic::Report) -> bool {
    let n = r.steps.len().saturating_sub(1);
    r.steps.iter().enumerate().any(|(k, s)| {
        s.note.contains("numerically") && !(k == n && s.note.contains("hecked numerically"))
    })
}

/// A Giac answer that is exact and passed its check.
fn exact(r: &symbolic::Report) -> bool {
    r.result.is_some()
        && !only_numeric(r)
        && r.steps
            .last()
            .is_some_and(|s| s.note.starts_with("Checked numerically"))
}

/// Simplify's answer, replaced by a tidier equivalent from Giac when there
/// is one, as one more step.
fn tidier(mut ours: symbolic::Report, latex: &str, others: &[String]) -> symbolic::Report {
    let Some(current) = ours.result.clone() else {
        return ours;
    };
    // `y = …` and `f(x) = …` are simplified on the right.
    let body = match crate_parse_is_relation(latex) {
        true => latex.split_once('=').map_or(latex, |(_, r)| r),
        false => latex,
    };
    let Ok(plan) = cas::plan(body, others, Some("giac:nicest")) else {
        return ours;
    };
    let answer = crate::giac::run(&plan);
    let Ok(theirs) = std::panic::catch_unwind(|| cas::finish(&plan, answer)) else {
        return ours;
    };
    let (Some(form), None) = (theirs.result, theirs.error) else {
        return ours;
    };
    if form.len() + 2 < current.len() {
        ours.steps.push(symbolic::Step {
            note: "Giac finds a tidier form, checked numerically".into(),
            latex: format!("={form}"),
        });
        ours.result = Some(form.clone());
        if ours.answer.is_some() {
            ours.answer = Some(match latex.split_once('=') {
                Some((lhs, _)) if crate_parse_is_relation(latex) => format!("{lhs}={form}"),
                _ => form,
            });
        }
    }
    ours
}

fn crate_parse_is_relation(latex: &str) -> bool {
    matches!(
        graph_expr::parse(latex),
        Ok(graph_expr::ast::Statement::Relation(..))
    )
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

/// The document's step check: for each group of lines, whether each line
/// follows from the one above, with the reason when it doesn't.
pub fn check_reply(request: &Value) -> Value {
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    let context = strings(&request["context"]);
    let groups: Vec<Value> = request["groups"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|g| {
            let lines = strings(g);
            let verdicts =
                std::panic::catch_unwind(|| graph_expr::check::check_group(&lines, &context))
                    .unwrap_or_else(|_| vec![graph_expr::check::Verdict::Unknown; lines.len()]);
            Value::Array(
                verdicts
                    .into_iter()
                    .map(|v| match v {
                        graph_expr::check::Verdict::Unknown => json!({ "status": "unknown" }),
                        graph_expr::check::Verdict::Follows => json!({ "status": "ok" }),
                        graph_expr::check::Verdict::Wrong(reason) => {
                            json!({ "status": "bad", "reason": reason })
                        }
                    })
                    .collect(),
            )
        })
        .collect();
    json!({ "seq": request["seq"], "groups": groups })
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

/// Which quantities a differential equation row plots, as the panel sends
/// them: `[across, up]`.
pub fn ode_axes(v: &serde_json::Value) -> Option<[usize; 2]> {
    let a = v.as_array()?;
    Some([a.first()?.as_u64()? as usize, a.get(1)?.as_u64()? as usize])
}

/// The starting values of a differential equation row's first two
/// quantities, when its axes leave them off.
pub fn ode_start(v: &serde_json::Value) -> [String; 2] {
    let at = |i: usize| v.get(i).and_then(|s| s.as_str()).unwrap_or("").to_owned();
    [at(0), at(1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_documents_steps() {
        let r = check_reply(&json!({
            "seq": 7,
            "groups": [["x^2=3x", "x=3"], ["\\left(x+1\\right)^2", "=x^2+2x+1"]],
            "context": []
        }));
        assert_eq!(r["seq"], 7);
        assert_eq!(r["groups"][0][0]["status"], "unknown");
        assert_eq!(r["groups"][0][1]["status"], "bad");
        assert_eq!(r["groups"][0][1]["reason"], "Lost the solution x = 0");
        assert_eq!(r["groups"][1][1]["status"], "ok");
    }

    #[test]
    fn replies_with_steps() {
        let rows = json!([{"id": 1, "latex": "f(x)=x^2-1"}, {"id": 2, "latex": "f(x)=3"}]);
        let r = reply(&rows, 2, "f(x)=3", None);
        assert_eq!(r["result"], "x=-2\\quad\\text{or}\\quad x=2");
        assert!(r["steps"].as_array().unwrap().len() > 1);
        assert_eq!(r["error"], Value::Null);
    }

    /// New kinds of problem: systems, inequalities, recurrences, matrices,
    /// Fourier transforms and assumptions. Needs the Giac worker.
    #[test]
    fn giac_problem_types() {
        if !crate::giac::available() {
            eprintln!("GRAPHCALC_GIAC isn't set; skipping");
            return;
        }
        let none = symbolic::Initial::default();
        let a = "A=\\begin{bmatrix}2&1\\\\1&2\\end{bmatrix}";
        let fact = "f\\left(n\\right)=\\left\\{n=0:1,2f\\left(n-1\\right)+1\\right\\}";
        let cases: &[(&str, &[&str], Option<&str>, &str)] = &[
            // Systems, in one row and with another row.
            ("x+y=3,x-y=1", &[], None, "x=2,\\ y=1"),
            ("x^{2}+y^{2}=25", &["y=x+1"], Some("giac:system"), "y=4"),
            // Inequalities Solve's own algebra can't do.
            ("-1<2x+1<5", &[], None, "-1<x<2"),
            (
                "\\sin\\left(x\\right)>2",
                &[],
                Some("giac:solve"),
                "No solutions",
            ),
            // Recurrences: a recursive function, and a subscript one.
            (fact, &[], None, "2^{n"),
            ("a_{n+1}=3a_{n}", &["a_{0}=2"], None, "3^{n}"),
            // Matrices.
            ("A", &[a], Some("giac:eigenvals"), "\\lambda="),
            ("A", &[a], Some("giac:eigenvects"), "P="),
            ("A", &[a], Some("giac:inv"), "\\frac{2}{3}"),
            ("A", &[a], Some("giac:det"), "3"),
            ("A", &[a], Some("giac:charpoly"), "\\lambda"),
            // Fourier.
            (
                "e^{-\\left|x\\right|}",
                &[],
                Some("giac:fourier"),
                "\\omega",
            ),
            // Exact where Solve's own answer was only numeric.
            ("e^{x}+x=5", &[], None, "W\\left(e^{5}\\right)"),
            // The tidiest form, from several of Giac's.
            ("\\frac{1}{x}+\\frac{1}{x+1}", &[], None, "\\frac{2x+1}"),
            (
                "\\frac{\\sin\\left(2x\\right)}{2\\cos\\left(x\\right)}",
                &[],
                None,
                "\\sin\\left(x\\right)",
            ),
            // Assumptions as a restriction.
            ("\\sqrt{x^{2}}\\left\\{x>0\\right\\}", &[], None, "x"),
            ("x^{2}=4\\left\\{x>0\\right\\}", &[], None, "x=2"),
        ];
        for (latex, others, op, want) in cases {
            let others: Vec<String> = others.iter().map(|s| s.to_string()).collect();
            let r = solve(latex, &others, *op, &none, true);
            let result = r.result.clone().unwrap_or_default();
            eprintln!(
                "{latex} -> {result:?} / {:?}",
                r.steps.last().map(|s| &s.note)
            );
            for st in &r.steps {
                eprintln!("    {} | {}", st.note, st.latex);
            }
            assert!(r.error.is_none(), "{latex}: {r:?}");
            assert!(result.contains(want), "{latex}: {result}");
            let last = &r.steps.last().unwrap().note;
            assert!(!last.contains("doesn't agree"), "{latex}: {r:?}");
        }
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
            ("\\frac{1}{s^{2}+1}", &[], Some("giac:ilaplace"), "\\sin"),
            (
                "y''+y=\\sin\\left(x\\right)",
                &[],
                Some("giac:desolve"),
                "c_{0}",
            ),
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
        // Degree mode: trig in degrees, checked in degrees.
        let deg: &[(&str, Option<&str>, &str)] = &[
            ("\\sin\\left(x\\right)=\\frac{1}{2}", None, "30"),
            ("\\frac{d}{dx}\\sin\\left(x\\right)", None, "180"),
            ("\\cos\\left(60\\right)", None, "\\frac{1}{2}"),
            ("\\arctan\\left(1\\right)", None, "45"),
        ];
        for (latex, op, want) in deg {
            let r = solve_in(latex, &[], *op, &none, true, true);
            let result = r.result.clone().unwrap_or_default();
            eprintln!("deg {latex} -> {result:?}");
            assert!(r.error.is_none(), "{latex}: {r:?}");
            assert!(result.contains(want), "{latex}: {result}");
            let last = &r.steps.last().unwrap().note;
            assert!(!last.contains("doesn't agree"), "{latex}: {r:?}");
        }
        // Infinite bounds and partial derivatives.
        let inf: &[(&str, &[&str], &str)] = &[
            (
                "\\prod_{n=2}^{\\infty}\\left(1-\\frac{1}{n^{2}}\\right)",
                &[],
                "\\frac{1}{2}",
            ),
            (
                "\\prod_{n=1}^{\\infty}\\frac{4n^{2}}{4n^{2}-1}",
                &[],
                "\\pi",
            ),
            ("\\sum_{n=0}^{\\infty}\\frac{1}{n!}", &[], "e"),
            (
                "\\sum_{n=1}^{\\infty}\\frac{1}{n}",
                &[],
                "\\text{diverges to }\\infty",
            ),
            (
                "\\sum_{n=0}^{\\infty}\\left(-1\\right)^{n}",
                &[],
                "\\text{diverges}",
            ),
            ("\\sum_{n=-\\infty}^{\\infty}e^{-n^{2}}", &[], "1.7726372"),
            (
                "\\prod_{n=1}^{\\infty}\\left(1+\\frac{1}{n}\\right)",
                &[],
                "\\text{diverges to }\\infty",
            ),
            (
                "\\int_{0}^{\\infty}\\frac{\\sin x}{x}dx",
                &[],
                "\\frac{\\pi}{2}",
            ),
            ("\\int_{0}^{\\infty}\\sin xdx", &[], "\\text{diverges}"),
            ("\\int_{-1}^{1}\\frac{1}{x}dx", &[], "\\text{diverges}"),
            ("\\lim_{x\\to\\infty}\\sin x", &[], "\\text{does not exist}"),
            (
                "\\frac{\\partial}{\\partial y}f\\left(x,y\\right)",
                &["f\\left(x,y\\right)=x^{2}y^{3}"],
                "3x^{2}y^{2}",
            ),
        ];
        for (latex, others, want) in inf {
            let others: Vec<String> = others.iter().map(|s| s.to_string()).collect();
            let r = solve(latex, &others, None, &none, true);
            let result = r.result.clone().unwrap_or_default();
            eprintln!("{latex} -> {result:?}");
            assert!(r.error.is_none(), "{latex}: {r:?}");
            assert!(result.contains(want), "{latex}: {result}");
            let last = &r.steps.last().unwrap().note;
            assert!(!last.contains("doesn't agree"), "{latex}: {r:?}");
        }
        // No trig: Solve's own algebra, as in radians.
        let r = solve_in("x^{2}=4", &[], None, &none, true, true);
        assert_eq!(r.result.as_deref(), Some("x=-2\\quad\\text{or}\\quad x=2"));
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
