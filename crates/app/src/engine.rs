//! Glue between the panel and the graph: takes the rows the panel sends,
//! analyzes them with graph-expr, keeps the curves to draw and builds the
//! reply the panel shows (errors, values, slider offers). Kept free of
//! window code so it is tested on every platform.

use graph_core::scene::{DEFAULT_CURVE_WIDTH, DEFAULT_POINT_SIZE};
use graph_core::{Axis, Curve, Field, Implicit, LineStyle, Parametric, Plot, PointStyle, Points};
use graph_expr::format::result_latex;
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

/// One parametric or polar curve ready to draw.
pub struct GraphParametric {
    pub x: Program,
    pub y: Program,
    pub domain: (f64, f64),
    pub color: [f32; 4],
    pub width: f32,
    pub style: LineStyle,
}

/// A row's points ready to draw.
pub struct GraphPoints {
    pub points: Vec<[f64; 2]>,
    pub color: [f32; 4],
    pub size: f32,
    pub style: PointStyle,
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
    pub parametrics: Vec<GraphParametric>,
    pub regions: Vec<GraphRegion>,
    pub points: Vec<GraphPoints>,
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
                domain: match (r["domain"][0].as_str(), r["domain"][1].as_str()) {
                    (Some(lo), Some(hi)) => Some([lo.to_owned(), hi.to_owned()]),
                    _ => None,
                },
            })
            .collect();
        let results = analyze(&inputs);

        self.latex = inputs.iter().map(|r| r.latex.clone()).collect();
        self.curves.clear();
        self.parametrics.clear();
        self.regions.clear();
        self.points.clear();
        for (row, result) in rows.iter().zip(&results) {
            if row["hidden"].as_bool().unwrap_or(false) {
                continue;
            }
            let (color, width, style) = (style_color(row), style_width(row), style_line(row));
            for curve in &result.curves {
                self.curves.push(GraphCurve {
                    program: curve.program.clone(),
                    axis: match curve.axis {
                        graph_expr::Axis::X => Axis::X,
                        graph_expr::Axis::Y => Axis::Y,
                    },
                    color,
                    width,
                    style,
                });
            }
            for p in &result.parametrics {
                self.parametrics.push(GraphParametric {
                    x: p.x.clone(),
                    y: p.y.clone(),
                    domain: p.domain,
                    color,
                    width,
                    style,
                });
            }
            for region in &result.regions {
                self.regions.push(GraphRegion {
                    shader: wgsl::body(&region.program),
                    program: region.program.clone(),
                    compare: region.compare,
                    color,
                    width,
                    style,
                });
            }
            if !result.points.is_empty() {
                self.points.push(GraphPoints {
                    points: result.points.clone(),
                    color,
                    size: point_size(row),
                    style: point_style(row),
                });
            }
        }
        Value::Array(results.iter().map(row_json).collect())
    }

    /// Calls `draw` with everything to plot, in the form graph-core takes.
    pub fn with_plot<R>(&self, draw: impl FnOnce(&Plot) -> R) -> R {
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
        let sweeps: Vec<_> = self
            .parametrics
            .iter()
            .map(|p| (move |t: f64| p.x.eval(t), move |t: f64| p.y.eval(t)))
            .collect();
        let parametrics: Vec<Parametric> = self
            .parametrics
            .iter()
            .zip(&sweeps)
            .map(|(p, (x, y))| Parametric {
                x,
                y,
                domain: p.domain,
                color: p.color,
                width: p.width,
                style: p.style,
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
        let points: Vec<Points> = self
            .points
            .iter()
            .map(|p| Points {
                points: &p.points,
                color: p.color,
                size: p.size,
                style: p.style,
            })
            .collect();
        draw(&Plot {
            curves: &curves,
            parametrics: &parametrics,
            implicits: &implicits,
            points: &points,
        })
    }

    /// How many things are drawn, for the smoke test's report.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn drawn(&self) -> usize {
        self.curves.len() + self.parametrics.len() + self.regions.len() + self.points.len()
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

fn point_size(row: &Value) -> f32 {
    row["pointSize"]
        .as_f64()
        .map_or(DEFAULT_POINT_SIZE, |s| s.clamp(1.0, 40.0) as f32)
}

fn point_style(row: &Value) -> PointStyle {
    match row["pointStyle"].as_str() {
        Some("open") => PointStyle::Open,
        Some("cross") => PointStyle::Cross,
        _ => PointStyle::Filled,
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
        "value": r.value.as_ref().map(result_latex),
        "slider": r.slider.filter(|v| v.is_finite()),
        "missing": r.missing.iter().map(|n| json!({"name": n, "latex": name_latex(n)})).collect::<Vec<_>>(),
        "graphed": r.graphed(),
        "points": !r.points.is_empty(),
        "lines": !(r.curves.is_empty() && r.parametrics.is_empty() && r.regions.is_empty()),
        "parameter": r.parameter.as_ref().map(|p| json!({
            "name": p.name,
            "latex": name_latex(p.name),
            "default": match p.name {
                "theta" => ["0", "12\\pi"],
                _ => ["0", "1"],
            },
            "error": p.error,
        })),
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
        engine.with_plot(|plot| assert_eq!((plot.curves[0].f)(1.5), 3.0));
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
        engine.with_plot(|plot| {
            let (curves, implicits) = (plot.curves, plot.implicits);
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
        engine.with_plot(|plot| assert_eq!(plot.implicits.len(), 1));
    }

    #[test]
    fn parametrics_points_and_lists() {
        let mut engine = Engine::default();
        let reply = engine.update(&json!([
            {"id": 1, "latex": "\\left(\\cos t,\\sin t\\right)", "domain": ["0", "2\\pi"]},
            {"id": 2, "latex": "r=\\theta"},
            {"id": 3, "latex": "\\left[\\left(1,2\\right),\\left(3,4\\right)\\right]", "pointStyle": "open", "pointSize": 12},
            {"id": 4, "latex": "y=\\left[1,2\\right]x"},
            {"id": 5, "latex": "\\left[1,2,3\\right]^2"},
        ]));
        assert_eq!(engine.parametrics.len(), 2);
        assert!((engine.parametrics[0].domain.1 - std::f64::consts::TAU).abs() < 1e-12);
        assert_eq!(reply[0]["parameter"]["name"], "t");
        assert_eq!(reply[1]["parameter"]["default"][1], "12\\pi");
        assert_eq!(engine.points.len(), 1);
        assert_eq!(engine.points[0].points.len(), 2);
        assert_eq!(engine.points[0].style, PointStyle::Open);
        assert_eq!(engine.points[0].size, 12.0);
        assert_eq!(reply[2]["points"], true);
        assert_eq!(reply[2]["lines"], false);
        assert_eq!(engine.curves.len(), 2, "one curve per list element");
        assert_eq!(reply[4]["value"], "\\left[1,4,9\\right]");
        assert_eq!(engine.drawn(), 5);
        engine.with_plot(|plot| {
            assert_eq!(plot.parametrics.len(), 2);
            assert_eq!(plot.points[0].points.len(), 2);
        });
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
