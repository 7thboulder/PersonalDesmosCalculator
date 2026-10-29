//! The Solve button: runs symbolic algebra on one row when asked, never
//! while typing, and sends the working back to the panel.

use graph_expr::symbolic;
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
    let latex = latex.to_owned();
    let op = op.map(str::to_owned);
    // A bug in the algebra must not take the window down with it.
    let report = std::panic::catch_unwind(move || {
        symbolic::run_row(&latex, &others, op.as_deref(), &initial)
    })
    .unwrap_or_else(|_| symbolic::Report {
        error: Some("Something went wrong while solving this".into()),
        ..Default::default()
    });
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
}
