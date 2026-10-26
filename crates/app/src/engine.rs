//! Glue between the panel and the graph: takes the rows the panel sends,
//! analyzes them with graph-expr, keeps the curves to draw and builds the
//! reply the panel shows (errors, values, slider offers). Kept free of
//! window code so it is tested on every platform.

use graph_core::scene::{DEFAULT_CURVE_WIDTH, DEFAULT_POINT_SIZE};
use graph_core::{
    Axis, Curve, Field, Implicit, LineStyle, Mark, Parametric, Plot, PointStyle, Points, Polygon,
};
use graph_expr::format::{exact_latex, result_latex, value_latex};
use graph_expr::{
    analyze_cached, wgsl, Cache, ColumnInput, Compare, Interval, Program, RowAnalysis, RowInput,
    RowKind,
};
use graph_render::{Shading, Test};
use serde_json::{json, Value};
use std::collections::HashMap;

/// Combines two hashes into one.
fn mix(a: u64, b: u64) -> u64 {
    (a ^ b.wrapping_mul(0x9e37_79b9_7f4a_7c15)).rotate_left(23)
}

/// How much of the colour Desmos lays down inside an inequality.
const SHADE_OPACITY: f32 = 0.4;
/// And inside a polygon, unless the row says otherwise.
const POLYGON_FILL: f64 = 0.4;

/// One curve ready to draw.
pub struct GraphCurve {
    /// The panel row it came from.
    pub row: u64,
    pub program: Program,
    pub axis: Axis,
    /// Changes when the curve does, so its drawn lines can be kept.
    pub key: u64,
    pub color: [f32; 4],
    pub width: f32,
    pub style: LineStyle,
}

/// One parametric or polar curve ready to draw.
pub struct GraphParametric {
    /// The panel row it came from.
    pub row: u64,
    pub x: Program,
    pub y: Program,
    pub domain: (f64, f64),
    pub color: [f32; 4],
    pub width: f32,
    pub style: LineStyle,
    /// As [`GraphCurve::key`].
    pub key: u64,
}

/// A row's points ready to draw.
pub struct GraphPoints {
    pub row: u64,
    pub points: Vec<[f64; 2]>,
    pub color: [f32; 4],
    pub size: f32,
    pub style: PointStyle,
    pub dots: bool,
    /// Joins the points in order: width and pattern.
    pub line: Option<(f32, LineStyle)>,
    /// The slider variables dragging the point moves, for `(a, b)`.
    pub drag: Option<[Option<String>; 2]>,
}

impl GraphPoints {
    /// Points styled by a row or table column's settings.
    fn styled(row: u64, points: Vec<[f64; 2]>, style: &Value) -> GraphPoints {
        GraphPoints {
            row,
            points,
            color: style_color(style),
            size: point_size(style),
            style: point_style(style),
            dots: style["dots"].as_bool().unwrap_or(true),
            line: style["connect"]
                .as_bool()
                .unwrap_or(false)
                .then(|| (style_width(style), style_line(style))),
            drag: None,
        }
    }
}

/// One polygon ready to draw.
pub struct GraphPolygon {
    pub points: Vec<[f64; 2]>,
    pub color: [f32; 4],
    pub fill: f32,
    pub width: f32,
    pub style: LineStyle,
}

/// One implicit equation or inequality ready to draw: its boundary is
/// traced on the CPU, and the shading, when it has any, on the GPU.
pub struct GraphRegion {
    /// The panel row it came from.
    pub row: u64,
    pub program: Program,
    pub compare: Compare,
    pub color: [f32; 4],
    pub width: f32,
    pub style: LineStyle,
    /// The relation as WGSL, when the GPU can evaluate it.
    pub shader: Option<String>,
    /// As [`GraphCurve::key`].
    pub key: u64,
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
    pub polygons: Vec<GraphPolygon>,
    /// The LaTeX of every row, in order, as last received.
    pub latex: Vec<String>,
    /// The row selected in the panel, whose curves show points of interest.
    pub selected: Option<u64>,
    /// The row defining each slider variable.
    pub sliders: HashMap<String, u64>,
    /// Bumped on every update, so cached work knows the rows changed.
    pub generation: u64,
    /// Variable values from the last update, reused where unchanged.
    cache: Cache,
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
                table: r["table"].as_array().map(|columns| {
                    columns
                        .iter()
                        .map(|c| ColumnInput {
                            header: c["header"].as_str().unwrap_or("").to_owned(),
                            cells: c["cells"]
                                .as_array()
                                .map_or(&[][..], Vec::as_slice)
                                .iter()
                                .map(|cell| cell.as_str().unwrap_or("").to_owned())
                                .collect(),
                        })
                        .collect()
                }),
            })
            .collect();
        let results = analyze_cached(&inputs, &mut self.cache);

        self.latex = inputs.iter().map(|r| r.latex.clone()).collect();
        self.generation += 1;
        self.sliders = results
            .iter()
            .filter_map(|r| match (&r.kind, r.slider) {
                (RowKind::Variable(name), Some(_)) => Some((name.clone(), r.id)),
                _ => None,
            })
            .collect();
        self.curves.clear();
        self.parametrics.clear();
        self.regions.clear();
        self.points.clear();
        self.polygons.clear();
        for (row, result) in rows.iter().zip(&results) {
            if row["hidden"].as_bool().unwrap_or(false) {
                continue;
            }
            let (color, width, style) = (style_color(row), style_width(row), style_line(row));
            for curve in &result.curves {
                self.curves.push(GraphCurve {
                    row: result.id,
                    program: curve.program.clone(),
                    axis: match curve.axis {
                        graph_expr::Axis::X => Axis::X,
                        graph_expr::Axis::Y => Axis::Y,
                    },
                    key: mix(curve.program.fingerprint(), curve.axis as u64),
                    color,
                    width,
                    style,
                });
            }
            for p in &result.parametrics {
                self.parametrics.push(GraphParametric {
                    row: result.id,
                    x: p.x.clone(),
                    y: p.y.clone(),
                    domain: p.domain,
                    color,
                    width,
                    style,
                    key: mix(
                        mix(p.x.fingerprint(), p.y.fingerprint()),
                        mix(p.domain.0.to_bits(), p.domain.1.to_bits()),
                    ),
                });
            }
            for region in &result.regions {
                self.regions.push(GraphRegion {
                    row: result.id,
                    shader: wgsl::body(&region.program),
                    program: region.program.clone(),
                    key: region.program.fingerprint(),
                    compare: region.compare,
                    color,
                    width,
                    style,
                });
            }
            for polygon in &result.polygons {
                self.polygons.push(GraphPolygon {
                    points: polygon.clone(),
                    color,
                    fill: row["fillOpacity"]
                        .as_f64()
                        .unwrap_or(POLYGON_FILL)
                        .clamp(0.0, 1.0) as f32,
                    width,
                    style,
                });
            }
            if !result.points.is_empty() {
                let mut points = GraphPoints::styled(result.id, result.points.clone(), row);
                points.drag = result.drag.clone();
                self.points.push(points);
            }
            // Each table column has its own colour and style.
            let columns = row["table"].as_array().map_or(&[][..], Vec::as_slice);
            for (style, column) in columns.iter().zip(&result.columns) {
                if !column.points.is_empty() && !style["hidden"].as_bool().unwrap_or(false) {
                    self.points
                        .push(GraphPoints::styled(result.id, column.points.clone(), style));
                }
            }
        }
        // Actions name the variables they set; the panel rewrites the rows
        // that define them.
        let defined: HashMap<&str, u64> = results
            .iter()
            .filter_map(|r| match &r.kind {
                RowKind::Variable(name) => Some((name.as_str(), r.id)),
                _ => None,
            })
            .collect();
        Value::Array(results.iter().map(|r| row_json(r, &defined)).collect())
    }

    /// Calls `draw` with everything to plot, in the form graph-core takes.
    pub fn with_plot<R>(&self, draw: impl FnOnce(&Plot) -> R) -> R {
        self.with_plot_marks(&[], draw)
    }

    /// As `with_plot`, with marks (points of interest, a trace point) on top.
    pub fn with_plot_marks<R>(&self, marks: &[Mark], draw: impl FnOnce(&Plot) -> R) -> R {
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
                key: Some(c.key),
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
                key: Some(p.key),
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
                key: Some(r.key),
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
                dots: p.dots,
                line: p.line,
            })
            .collect();
        let polygons: Vec<Polygon> = self
            .polygons
            .iter()
            .map(|p| Polygon {
                points: &p.points,
                color: p.color,
                fill: p.fill,
                width: p.width,
                style: p.style,
            })
            .collect();
        draw(&Plot {
            curves: &curves,
            parametrics: &parametrics,
            implicits: &implicits,
            polygons: &polygons,
            points: &points,
            marks,
        })
    }

    /// How many things are drawn, for the smoke test's report.
    #[cfg_attr(not(windows), allow(dead_code))]
    pub fn drawn(&self) -> usize {
        self.curves.len()
            + self.parametrics.len()
            + self.regions.len()
            + self.points.len()
            + self.polygons.len()
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

fn row_json(r: &RowAnalysis, defined: &HashMap<&str, u64>) -> Value {
    let (kind, name) = match &r.kind {
        RowKind::Empty => ("empty", None),
        RowKind::Variable(n) => ("variable", Some(n)),
        RowKind::Function(n) => ("function", Some(n)),
        RowKind::Curve => ("curve", None),
        RowKind::Table => ("table", None),
        RowKind::Value => ("value", None),
        RowKind::Action => ("action", None),
        RowKind::Regression => ("regression", None),
        RowKind::NotYet => ("notyet", None),
        RowKind::Error => ("error", None),
    };
    json!({
        "id": r.id,
        "kind": kind,
        "name": name,
        "error": r.error,
        "value": r.value.as_ref().map(result_latex),
        // A regression's parameters and R², as Desmos lists them under it.
        "fit": r.fit.as_ref().map(|f| {
            let mut parts: Vec<String> = f
                .params
                .iter()
                .map(|(n, v)| format!("{}={}", name_latex(n), value_latex(fitted(*v))))
                .collect();
            if let Some(r2) = f.r2 {
                parts.push(format!("R^{{2}}={}", value_latex(fitted(r2))));
            }
            parts.join("\\quad ")
        }),
        "slider": r.slider.filter(|v| v.is_finite()),
        "missing": r.missing.iter().map(|n| json!({"name": n, "latex": name_latex(n)})).collect::<Vec<_>>(),
        "graphed": r.graphed(),
        "points": !r.points.is_empty(),
        "polygons": !r.polygons.is_empty(),
        "updates": r.updates.iter().filter_map(|(name, v)| Some(json!({
            "row": defined.get(name.as_str())?,
            "latex": exact_latex(v),
        }))).collect::<Vec<_>>(),
        "lines": !(r.curves.is_empty() && r.parametrics.is_empty() && r.regions.is_empty()),
        "columns": r.columns.iter().map(|c| json!({
            "error": c.error,
            "missing": c.missing,
            "computed": c.computed.as_ref().map(|vs| vs
                .iter()
                .map(|v| if v.is_finite() { value_latex(*v) } else { String::new() })
                .collect::<Vec<_>>()),
            "cellErrors": c.cell_errors,
            "points": !c.points.is_empty(),
        })).collect::<Vec<_>>(),
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
/// Fitted values carry least-squares noise in the last few digits, so they
/// are shown to 10 significant figures, as Desmos rounds them.
fn fitted(v: f64) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return v;
    }
    format!("{v:.9e}").parse().unwrap_or(v)
}

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
    fn polygons_draw_and_actions_name_their_rows() {
        let mut engine = Engine::default();
        let reply = engine.update(&json!([
            {"id": 1, "latex": "a=0.1"},
            {"id": 2, "latex": "a\\to a+0.2,L\\to\\operatorname{join}\\left(L,a\\right)"},
            {"id": 3, "latex": "L=\\left[1\\right]"},
            {"id": 4, "latex": "\\operatorname{polygon}\\left(\\left(0,0\\right),\\left(1,0\\right),\\left(0,a\\right)\\right)",
             "fillOpacity": 0.25},
            {"id": 5, "latex": "a\\to1", "hidden": true},
        ]));
        assert_eq!(reply[1]["kind"], "action");
        // Exact values, so running an action never loses precision.
        assert_eq!(
            reply[1]["updates"],
            json!([
                {"row": 1, "latex": "0.30000000000000004"},
                {"row": 3, "latex": "\\left[1,0.1\\right]"},
            ])
        );
        assert_eq!(reply[3]["polygons"], true);
        assert_eq!(engine.polygons.len(), 1);
        assert_eq!(
            engine.polygons[0].points,
            vec![[0.0, 0.0], [1.0, 0.0], [0.0, 0.1]]
        );
        assert_eq!(engine.polygons[0].fill, 0.25);
        assert_eq!(
            reply[4]["updates"],
            json!([{"row": 1, "latex": "1"}]),
            "hidden rows still run"
        );
    }

    #[test]
    fn table_columns_draw_with_their_own_styles() {
        let mut engine = Engine::default();
        let reply = engine.update(&json!([
            {"id": 1, "latex": "", "table": [
                {"header": "x_1", "cells": ["1", "2", "3", ""]},
                {"header": "y_1", "cells": ["4", "", "6", ""], "color": "#388c46", "connect": true},
                {"header": "x_1^2", "cells": [], "dots": false, "connect": true, "style": "dashed"},
                {"header": "y_2", "cells": ["1", "1", "1"], "hidden": true},
            ]},
        ]));
        assert_eq!(reply[0]["kind"], "table");
        assert_eq!(reply[0]["columns"][2]["computed"], json!(["1", "4", "9"]));
        assert_eq!(engine.points.len(), 2, "the hidden column is not drawn");
        let (y1, sq) = (&engine.points[0], &engine.points[1]);
        assert_eq!(y1.points, vec![[1.0, 4.0], [3.0, 6.0]]);
        assert!(y1.dots && y1.line == Some((DEFAULT_CURVE_WIDTH, LineStyle::Solid)));
        assert_eq!(y1.color[1], 0x8c as f32 / 255.0);
        assert!(!sq.dots && sq.line.map(|l| l.1) == Some(LineStyle::Dashed));
    }

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
