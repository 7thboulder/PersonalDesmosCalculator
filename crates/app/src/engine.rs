//! Glue between the panel and the graph: takes the rows the panel sends,
//! analyzes them with graph-expr, keeps the curves to draw and builds the
//! reply the panel shows (errors, values, slider offers). Kept free of
//! window code so it is tested on every platform.

use graph_core::scene::DEFAULT_CURVE_WIDTH;
use graph_core::{Axis, Curve, LineStyle};
use graph_expr::format::value_latex;
use graph_expr::{analyze, Program, RowAnalysis, RowInput, RowKind};
use serde_json::{json, Value};

/// One curve ready to draw.
pub struct GraphCurve {
    pub program: Program,
    pub axis: Axis,
    pub color: [f32; 4],
    pub width: f32,
    pub style: LineStyle,
}

#[derive(Default)]
pub struct Engine {
    pub curves: Vec<GraphCurve>,
    /// The LaTeX of every row, in order, as last received.
    pub latex: Vec<String>,
}

impl Engine {
    /// Takes the panel's `rows` array, re-analyzes everything and returns the
    /// per-row results for the panel as a JSON array.
    pub fn update(&mut self, rows: &Value) -> Value {
        let rows: &[Value] = rows.as_array().map_or(&[], Vec::as_slice);
        let inputs: Vec<RowInput> = rows
            .iter()
            .map(|r| RowInput {
                id: r["id"].as_u64().unwrap_or(0),
                latex: r["latex"].as_str().unwrap_or("").to_owned(),
            })
            .collect();
        let results = analyze(&inputs);

        self.latex = inputs.iter().map(|r| r.latex.clone()).collect();
        self.curves = rows
            .iter()
            .zip(&results)
            .filter(|(row, _)| !row["hidden"].as_bool().unwrap_or(false))
            .filter_map(|(row, result)| {
                let curve = result.curve.as_ref()?;
                Some(GraphCurve {
                    program: curve.program.clone(),
                    axis: match curve.axis {
                        graph_expr::Axis::X => Axis::X,
                        graph_expr::Axis::Y => Axis::Y,
                    },
                    color: parse_color(
                        row["color"].as_str().unwrap_or("#c74440"),
                        row["opacity"].as_f64().unwrap_or(1.0),
                    ),
                    width: row["width"]
                        .as_f64()
                        .map_or(DEFAULT_CURVE_WIDTH, |w| w.clamp(0.0, 20.0) as f32),
                    style: match row["style"].as_str() {
                        Some("dashed") => LineStyle::Dashed,
                        Some("dotted") => LineStyle::Dotted,
                        _ => LineStyle::Solid,
                    },
                })
            })
            .collect();
        Value::Array(results.iter().map(row_json).collect())
    }

    /// Calls `draw` with the curves in the form graph-core takes.
    pub fn with_curves<R>(&self, draw: impl FnOnce(&[Curve]) -> R) -> R {
        let fns: Vec<_> = self
            .curves
            .iter()
            .map(|c| move |t: f64| c.program.eval(t))
            .collect();
        let curves: Vec<Curve> = self
            .curves
            .iter()
            .zip(&fns)
            .map(|(c, f)| Curve {
                f,
                axis: c.axis,
                color: c.color,
                width: c.width,
                style: c.style,
            })
            .collect();
        draw(&curves)
    }
}

fn row_json(r: &RowAnalysis) -> Value {
    let (kind, name) = match &r.kind {
        RowKind::Empty => ("empty", None),
        RowKind::Variable(n) => ("variable", Some(n)),
        RowKind::Function(n) => ("function", Some(n)),
        RowKind::Curve => ("curve", None),
        RowKind::Value => ("value", None),
        RowKind::NotYet => ("notyet", None),
        RowKind::Error => ("error", None),
    };
    json!({
        "id": r.id,
        "kind": kind,
        "name": name,
        "error": r.error,
        "value": r.value.map(value_latex),
        "slider": r.slider.filter(|v| v.is_finite()),
        "missing": r.missing.iter().map(|n| json!({"name": n, "latex": name_latex(n)})).collect::<Vec<_>>(),
        "graphed": r.curve.is_some(),
    })
}

/// How the panel types a name: `a_1` is `a_{1}`, `alpha` is `\alpha`.
pub fn name_latex(name: &str) -> String {
    let (base, sub) = match name.split_once('_') {
        Some((b, s)) => (b, Some(s)),
        None => (name, None),
    };
    let base = if base.chars().count() > 1 {
        format!("\\{base}")
    } else {
        base.to_owned()
    };
    match sub {
        Some(s) => format!("{base}_{{{s}}}"),
        None => base,
    }
}

/// `#rrggbb` to RGBA floats in 0..1; anything unreadable is Desmos red.
pub fn parse_color(hex: &str, opacity: f64) -> [f32; 4] {
    let hex = hex.trim_start_matches('#');
    let channel = |i: usize| {
        hex.get(i..i + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .map(|v| v as f32 / 255.0)
    };
    let alpha = opacity.clamp(0.0, 1.0) as f32;
    match (hex.len(), channel(0), channel(2), channel(4)) {
        (6, Some(r), Some(g), Some(b)) => [r, g, b, alpha],
        _ => [
            0xc7 as f32 / 255.0,
            0x44 as f32 / 255.0,
            0x40 as f32 / 255.0,
            alpha,
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_become_curves_and_panel_results() {
        let mut engine = Engine::default();
        let rows = json!([
            {"id": 1, "latex": "a=2", "color": "#2d70b3"},
            {"id": 2, "latex": "y=ax", "color": "#2d70b3", "style": "dashed", "width": 4},
            {"id": 3, "latex": "x=y^2", "color": "#388c46", "hidden": true},
            {"id": 4, "latex": "y=mx", "color": "#000000"},
            {"id": 5, "latex": "1+1"},
        ]);
        let reply = engine.update(&rows);
        assert_eq!(
            engine.curves.len(),
            1,
            "hidden and incomplete rows are not drawn"
        );
        let c = &engine.curves[0];
        assert_eq!(c.program.eval(3.0), 6.0);
        assert_eq!(c.style, LineStyle::Dashed);
        assert_eq!(c.width, 4.0);
        assert_eq!(c.color[2], 0xb3 as f32 / 255.0);

        assert_eq!(reply[0]["kind"], "variable");
        assert_eq!(reply[0]["slider"], 2.0);
        assert_eq!(reply[1]["graphed"], true);
        assert_eq!(reply[2]["graphed"], true, "hidden rows still analyze");
        assert_eq!(reply[3]["missing"][0]["name"], "m");
        assert_eq!(reply[4]["value"], "2");
        engine.with_curves(|curves| assert_eq!((curves[0].f)(1.5), 3.0));
    }

    #[test]
    fn names_and_colors() {
        assert_eq!(name_latex("a"), "a");
        assert_eq!(name_latex("a_1"), "a_{1}");
        assert_eq!(name_latex("alpha"), "\\alpha");
        assert_eq!(parse_color("#ffffff", 0.5), [1.0, 1.0, 1.0, 0.5]);
        assert_eq!(parse_color("nope", 1.0)[0], 0xc7 as f32 / 255.0);
    }
}
