//! Glue between the panel and the graph: takes the rows the panel sends,
//! analyzes them with graph-expr, keeps the curves to draw and builds the
//! reply the panel shows (errors, values, slider offers). Kept free of
//! window code so it is tested on every platform.

use graph_core::scene::DEFAULT_CURVE_WIDTH;
use graph_core::{Axis, Curve, Field, Implicit, LineStyle};
use graph_expr::format::value_latex;
use graph_expr::{analyze, wgsl, Compare, Interval, Program, RowAnalysis, RowInput, RowKind};
use graph_render::{Shading, Test};
use serde_json::{json, Value};

/// How much of the colour Desmos lays down inside an inequality.
const SHADE_OPACITY: f32 = 0.4;

/// One curve ready to draw.
pub struct GraphCurve {
    pub program: Program,
    pub axis: Axis,
    pub color: [f32; 4],
    pub width: f32,
    pub style: LineStyle,
}

/// One implicit equation or inequality ready to draw: its boundary is
/// traced on the CPU, and the shading, when it has any, on the GPU.
pub struct GraphRegion {
    pub program: Program,
    pub compare: Compare,
    pub color: [f32; 4],
    pub width: f32,
    pub style: LineStyle,
    /// The relation as WGSL, when the GPU can evaluate it.
    pub shader: Option<String>,
}

impl GraphRegion {
    /// Whether the GPU draws this boundary. It can when it has the relation
    /// as a shader and the line is solid; a dashed or dotted boundary has
    /// to be traced on the CPU, which gives a path to run the pattern along.
    fn boundary_on_gpu(&self) -> bool {
        self.shader.is_some() && self.style == LineStyle::Solid
    }
}

/// A region's field, in the form graph-core traces.
struct RegionField<'a>(&'a Program);

impl Field for RegionField<'_> {
    fn value(&self, x: f64, y: f64) -> f64 {
        self.0.eval2(x, y)
    }

    fn range(&self, x: (f64, f64), y: (f64, f64)) -> Option<(f64, f64)> {
        let bound = self
            .0
            .eval_interval(Interval::new(x.0, x.1), Interval::new(y.0, y.1));
        (!bound.is_empty()).then_some((bound.lo, bound.hi))
    }
}

#[derive(Default)]
pub struct Engine {
    pub curves: Vec<GraphCurve>,
    pub regions: Vec<GraphRegion>,
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
        let visible = |row: &&Value| !row["hidden"].as_bool().unwrap_or(false);
        self.regions = rows
            .iter()
            .zip(&results)
            .filter(|(row, _)| visible(row))
            .filter_map(|(row, result)| {
                let region = result.region.as_ref()?;
                Some(GraphRegion {
                    shader: wgsl::body(&region.program),
                    program: region.program.clone(),
                    compare: region.compare,
                    color: style_color(row),
                    width: style_width(row),
                    style: style_line(row),
                })
            })
            .collect();
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
                    color: style_color(row),
                    width: style_width(row),
                    style: style_line(row),
                })
            })
            .collect();
        Value::Array(results.iter().map(row_json).collect())
    }

    /// Calls `draw` with everything to plot, in the form graph-core takes.
    pub fn with_plot<R>(&self, draw: impl FnOnce(&[Curve], &[Implicit]) -> R) -> R {
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
        // Only the boundaries the GPU cannot draw are traced here.
        let traced: Vec<&GraphRegion> = self
            .regions
            .iter()
            .filter(|r| !r.boundary_on_gpu())
            .collect();
        let fields: Vec<RegionField> = traced.iter().map(|r| RegionField(&r.program)).collect();
        let implicits: Vec<Implicit> = traced
            .iter()
            .zip(&fields)
            .map(|(r, field)| Implicit {
                field,
                color: r.color,
                width: r.width,
                style: r.style,
            })
            .collect();
        draw(&curves, &implicits)
    }

    /// What the GPU draws for this frame: the fill inside every inequality
    /// it can evaluate, and the boundary of every solid-styled one.
    /// `origin` is the world position of the target's pixel (0, 0), and
    /// `scale` the display's DPI scale.
    pub fn shadings(&self, origin: [f64; 2], units_per_px: f64, scale: f32) -> Vec<Shading> {
        self.regions
            .iter()
            .filter_map(|r| {
                let test = match r.compare {
                    Compare::Lt => Some(Test::Less),
                    Compare::Le => Some(Test::LessOrEqual),
                    Compare::Gt => Some(Test::Greater),
                    Compare::Ge => Some(Test::GreaterOrEqual),
                    Compare::Eq => None,
                };
                let line_px = if r.boundary_on_gpu() {
                    r.width * scale
                } else {
                    0.0
                };
                if test.is_none() && line_px == 0.0 {
                    return None; // Nothing here for the GPU to do.
                }
                Some(Shading {
                    body: r.shader.clone()?,
                    test,
                    color: r.color,
                    fill_opacity: SHADE_OPACITY,
                    line_px,
                    origin: [origin[0] as f32, origin[1] as f32],
                    units_per_px: units_per_px as f32,
                })
            })
            .collect()
    }
}

fn style_color(row: &Value) -> [f32; 4] {
    parse_color(
        row["color"].as_str().unwrap_or("#c74440"),
        row["opacity"].as_f64().unwrap_or(1.0),
    )
}

fn style_width(row: &Value) -> f32 {
    row["width"]
        .as_f64()
        .map_or(DEFAULT_CURVE_WIDTH, |w| w.clamp(0.0, 20.0) as f32)
}

fn style_line(row: &Value) -> LineStyle {
    match row["style"].as_str() {
        Some("dashed") => LineStyle::Dashed,
        Some("dotted") => LineStyle::Dotted,
        _ => LineStyle::Solid,
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
        "graphed": r.curve.is_some() || r.region.is_some(),
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
        engine.with_plot(|curves, _| assert_eq!((curves[0].f)(1.5), 3.0));
    }

    #[test]
    fn implicit_rows_become_regions_and_shadings() {
        let mut engine = Engine::default();
        let reply = engine.update(&json!([
            {"id": 1, "latex": "x^2+y^2=25", "color": "#c74440"},
            {"id": 2, "latex": "y>x^2", "color": "#2d70b3"},
            {"id": 3, "latex": "x!>y", "color": "#388c46"},
        ]));
        assert_eq!(engine.regions.len(), 3);
        assert_eq!(reply[0]["graphed"], true);
        assert_eq!(reply[1]["graphed"], true);

        // The circle and the parabola are drawn entirely by the GPU: the
        // equation as a line, the inequality as a line and a fill.
        let shadings = engine.shadings([-10.0, 10.0], 0.02, 1.0);
        assert_eq!(shadings.len(), 2);
        assert_eq!(shadings[0].test, None, "an equation has no inside");
        assert!(shadings[0].line_px > 0.0);
        assert_eq!(shadings[1].test, Some(Test::Greater));
        assert!((shadings[1].fill_opacity - 0.4).abs() < 1e-6);

        // A factorial has no GPU form, so that row is traced on the CPU.
        assert!(engine.regions[2].shader.is_none());
        engine.with_plot(|curves, implicits| {
            assert!(curves.is_empty());
            assert_eq!(implicits.len(), 1, "only what the GPU cannot draw");
            // x! > y at (3, 4): 3! - 4 = 2.
            assert_eq!(implicits[0].field.value(3.0, 4.0), 2.0);
        });

        // A dashed boundary goes back to the CPU, which can run the pattern
        // along the curve, while its fill stays on the GPU.
        let mut engine = Engine::default();
        engine.update(&json!([
            {"id": 1, "latex": "x^2+y^2<4", "color": "#c74440", "style": "dashed"},
        ]));
        assert_eq!(engine.shadings([-10.0, 10.0], 0.02, 1.0)[0].line_px, 0.0);
        engine.with_plot(|_, implicits| assert_eq!(implicits.len(), 1));
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
