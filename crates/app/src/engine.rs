//! Glue between the panel and the graph: takes the rows the panel sends,
//! analyzes them with graph-expr, keeps the curves to draw and builds the
//! reply the panel shows (errors, values, slider offers). Kept free of
//! window code so it is tested on every platform.

use graph_core::scene::{DEFAULT_CURVE_WIDTH, DEFAULT_POINT_SIZE};
use graph_core::three::{self, Mesh, View3};
use graph_core::{
    Axis, Curve, Field, Implicit, LineStyle, Mark, Parametric, Plot, PointStyle, Points, Polygon,
    SlopeField, TextAtlas, Vertex,
};
use graph_expr::format::{exact_latex, result_latex, value_latex};
use graph_expr::{
    analyze_mode, wgsl, Cache, ColumnInput, Compare, Interval, Mode, Program, RowAnalysis,
    RowInput, RowKind,
};
use graph_render::{Frame3, Shading, Test};
use serde_json::{json, Value};
use std::collections::HashMap;

// The renderer reserves room for exactly the numbers the WGSL reads.
const _: () = assert!(wgsl::MAX_CONSTANTS == graph_render::MAX_CONSTANTS);

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
    /// For a differential equation's initial point written as plain
    /// numbers: its place among the row's initial points, which dragging
    /// it rewrites.
    pub seed: Option<usize>,
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
            seed: None,
        }
    }
}

/// A differential equation y' = f(x, y) ready to draw: its slope field
/// and the solution through each initial point.
pub struct GraphOde {
    pub slope: Program,
    pub seeds: Vec<[f64; 2]>,
    pub color: [f32; 4],
    pub width: f32,
    pub style: LineStyle,
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
    pub shader: Option<wgsl::Shader>,
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

/// How opaque a shaded solid is by default, as a share of its row's
/// opacity. Its row's Fill slider changes it.
const SOLID_OPACITY: f32 = 0.4;

/// What a surface in a 3D graph is.
pub enum Shape {
    /// `z = f(x, y)`.
    Explicit(Program),
    /// `f(x, y, z) = 0`.
    Implicit(Program),
    /// The translucent solid where `f(x, y, z) < 0`, or `> 0` when
    /// `above`.
    Solid { program: Program, above: bool },
    /// A polygon's fill, by its corners.
    Polygon(Vec<[f64; 3]>),
    /// A shaded box: its low and high end on each axis.
    Box([[f64; 2]; 3]),
    /// `(x(u, v), y(u, v), z(u, v))`.
    Parametric {
        x: Program,
        y: Program,
        z: Program,
        u: (f64, f64),
        v: (f64, f64),
    },
}

/// One surface ready to draw in a 3D graph.
pub struct GraphSurface {
    pub shape: Shape,
    pub color: [f32; 4],
    /// As [`GraphCurve::key`].
    pub key: u64,
}

/// A row's points in a 3D graph.
pub struct GraphPoints3 {
    pub points: Vec<[f64; 3]>,
    pub color: [f32; 4],
    pub size: f32,
}

/// A polygon's outline in a 3D graph (its fill is a surface).
pub struct GraphPolygon3 {
    pub corners: Vec<[f64; 3]>,
    pub color: [f32; 4],
    pub width: f32,
}

/// A curve in a 3D graph, traced by t.
pub struct GraphCurve3 {
    pub x: Program,
    pub y: Program,
    pub z: Program,
    pub domain: (f64, f64),
    pub color: [f32; 4],
    pub width: f32,
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
    pub odes: Vec<GraphOde>,
    pub surfaces: Vec<GraphSurface>,
    pub points3: Vec<GraphPoints3>,
    pub curves3: Vec<GraphCurve3>,
    pub polygons3: Vec<GraphPolygon3>,
    /// Flat or 3D, as the panel last said.
    pub mode: Mode,
    /// Surface meshes for the current view, by surface and range.
    meshes: Vec<(u64, Mesh)>,
    /// The 3D points' and curves' lines, by rows and range. They are in box
    /// coordinates, so turning the box keeps them.
    lines3: Option<(u64, Vec<three::LineVertex>)>,
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
                seeds: r["seeds"].as_str().map(str::to_owned),
            })
            .collect();
        let results = analyze_mode(&inputs, &mut self.cache, self.mode);

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
        self.odes.clear();
        self.surfaces.clear();
        self.points3.clear();
        self.curves3.clear();
        self.polygons3.clear();
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
            let seeds: Vec<[f64; 2]> = result.seeds.iter().map(|s| s.at).collect();
            for slope in &result.odes {
                self.odes.push(GraphOde {
                    slope: slope.clone(),
                    seeds: seeds.clone(),
                    color,
                    width,
                    style,
                });
            }
            // Each initial point is a dot of its own, so it can be dragged.
            for (i, seed) in result.seeds.iter().enumerate() {
                let mut dot = GraphPoints::styled(result.id, vec![seed.at], row);
                dot.dots = true;
                dot.line = None;
                dot.drag = seed.drag.clone();
                dot.seed = result.seeds_fixed.then_some(i);
                self.points.push(dot);
            }
            let solid_fill = row["fillOpacity"]
                .as_f64()
                .map_or(SOLID_OPACITY, |f| f.clamp(0.0, 1.0) as f32);
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
            for surface in &result.surfaces {
                self.surfaces.push(GraphSurface {
                    shape: Shape::Explicit(surface.program.clone()),
                    key: surface.program.fingerprint(),
                    color,
                });
            }
            for surface in &result.implicits3 {
                let program = surface.program.clone();
                self.surfaces.push(match surface.shade {
                    None => GraphSurface {
                        shape: Shape::Implicit(program),
                        key: mix(surface.program.fingerprint(), 1),
                        color,
                    },
                    Some(compare) => {
                        let above = matches!(compare, Compare::Gt | Compare::Ge);
                        GraphSurface {
                            shape: Shape::Solid { program, above },
                            key: mix(surface.program.fingerprint(), 3 + above as u64),
                            color: [color[0], color[1], color[2], color[3] * solid_fill],
                        }
                    }
                });
            }
            for s in &result.param_surfaces {
                let bits = [s.u.0, s.u.1, s.v.0, s.v.1]
                    .iter()
                    .fold(2, |k, b| mix(k, b.to_bits()));
                self.surfaces.push(GraphSurface {
                    shape: Shape::Parametric {
                        x: s.x.clone(),
                        y: s.y.clone(),
                        z: s.z.clone(),
                        u: s.u,
                        v: s.v,
                    },
                    key: mix(
                        mix(s.x.fingerprint(), s.y.fingerprint()),
                        mix(s.z.fingerprint(), bits),
                    ),
                    color,
                });
            }
            if !result.points3.is_empty() {
                self.points3.push(GraphPoints3 {
                    points: result.points3.clone(),
                    color,
                    size: point_size(row),
                });
            }
            for c in &result.curves3 {
                self.curves3.push(GraphCurve3 {
                    x: c.x.clone(),
                    y: c.y.clone(),
                    z: c.z.clone(),
                    domain: c.domain,
                    color,
                    width,
                });
            }
            for b in &result.boxes3 {
                self.surfaces.push(GraphSurface {
                    shape: Shape::Box(*b),
                    key: b.iter().flatten().fold(6, |k, c| mix(k, c.to_bits())),
                    color: [color[0], color[1], color[2], color[3] * solid_fill],
                });
            }
            for corners in &result.polygons3 {
                let fill = row["fillOpacity"]
                    .as_f64()
                    .unwrap_or(POLYGON_FILL)
                    .clamp(0.0, 1.0) as f32;
                let key = corners.iter().flatten().fold(5, |k, c| mix(k, c.to_bits()));
                self.surfaces.push(GraphSurface {
                    shape: Shape::Polygon(corners.clone()),
                    key,
                    color: [color[0], color[1], color[2], color[3] * fill],
                });
                self.polygons3.push(GraphPolygon3 {
                    corners: corners.clone(),
                    color,
                    width,
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
        let slopes: Vec<_> = self
            .odes
            .iter()
            .map(|o| move |x: f64, y: f64| o.slope.eval2(x, y))
            .collect();
        let fields: Vec<SlopeField> = self
            .odes
            .iter()
            .zip(&slopes)
            .map(|(o, slope)| SlopeField {
                slope,
                seeds: &o.seeds,
                color: o.color,
                width: o.width,
                style: o.style,
            })
            .collect();
        draw(&Plot {
            curves: &curves,
            parametrics: &parametrics,
            implicits: &implicits,
            polygons: &polygons,
            points: &points,
            fields: &fields,
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
            + self.odes.len()
            + self.surfaces.len()
            + self.points3.len()
            + self.curves3.len()
    }

    /// Calls `draw` with a 3D frame of `view` and the 2D overlay (the
    /// box's numbers) to draw over it. Surface meshes are kept while the
    /// surface and the box's range stay the same, so turning the box
    /// costs nothing here.
    pub fn with_frame_3d<R>(
        &mut self,
        view: &View3,
        scale: f32,
        text: &mut TextAtlas,
        draw: impl FnOnce(&Frame3, &[Vertex], &TextAtlas) -> R,
    ) -> R {
        let range = view
            .center
            .iter()
            .chain(&view.half)
            .fold(0, |k, v| mix(k, v.to_bits()));
        let keys: Vec<u64> = self
            .surfaces
            .iter()
            .map(|s| {
                let color = s.color.iter().fold(0, |k, c| mix(k, c.to_bits() as u64));
                mix(mix(s.key, color), range)
            })
            .collect();
        let mut kept: Vec<(u64, Mesh)> = Vec::with_capacity(keys.len());
        for (s, &key) in self.surfaces.iter().zip(&keys) {
            let mesh = match self.meshes.iter().position(|(k, _)| *k == key) {
                Some(i) => self.meshes.swap_remove(i).1,
                None => match &s.shape {
                    Shape::Explicit(p) => {
                        let f = |x: f64, y: f64| p.eval2(x, y);
                        three::surface_mesh(view, &f, s.color, three::SURFACE_SAMPLES)
                    }
                    Shape::Implicit(p) => {
                        let f = |x: f64, y: f64, z: f64| p.eval3(x, y, z);
                        three::implicit_mesh(view, &f, s.color, three::IMPLICIT_SAMPLES)
                    }
                    Shape::Solid { program, above } => {
                        let sign = if *above { -1.0 } else { 1.0 };
                        let f = |x: f64, y: f64, z: f64| sign * program.eval3(x, y, z);
                        three::solid_mesh(view, &f, s.color, three::IMPLICIT_SAMPLES)
                    }
                    Shape::Polygon(corners) => three::polygon_mesh(view, corners, s.color),
                    Shape::Box(b) => three::box_mesh(view, *b, s.color),
                    Shape::Parametric { x, y, z, u, v } => {
                        let f = |a: f64, b: f64| [x.eval2(a, b), y.eval2(a, b), z.eval2(a, b)];
                        three::parametric_mesh(view, &f, *u, *v, s.color, three::PARAMETRIC_SAMPLES)
                    }
                },
            };
            kept.push((key, mesh));
        }
        self.meshes = kept;
        let meshes: Vec<&Mesh> = self.meshes.iter().map(|(_, m)| m).collect();
        let lines_key = mix(mix(self.generation, range), scale.to_bits() as u64);
        if !matches!(&self.lines3, Some((k, _)) if *k == lines_key) {
            let mut lines = Vec::new();
            for c in &self.curves3 {
                three::curve_3d(
                    view,
                    &|t| c.x.eval(t),
                    &|t| c.y.eval(t),
                    &|t| c.z.eval(t),
                    c.domain,
                    c.width * scale,
                    c.color,
                    &mut lines,
                );
            }
            for p in &self.polygons3 {
                three::polygon_outline_3d(view, &p.corners, p.width * scale, p.color, &mut lines);
            }
            for p in &self.points3 {
                three::points_3d(view, &p.points, p.size * scale, p.color, &mut lines);
            }
            self.lines3 = Some((lines_key, lines));
        }
        let lines = three::box_lines(view, scale);
        let marks = self.lines3.as_ref().map_or(&[][..], |(_, l)| &l[..]);
        let overlay = three::box_labels(view, scale, text);
        let step = three::steps(view);
        let grid = [
            (step[0] / view.half[0]) as f32,
            (step[1] / view.half[1]) as f32,
            (-view.center[0] / view.half[0]) as f32,
            (-view.center[1] / view.half[1]) as f32,
        ];
        let frame = Frame3 {
            matrix: view.matrix(),
            toward: view.toward().map(|v| v as f32),
            grid,
            surfaces: &meshes,
            surfaces_key: keys.iter().fold(keys.len() as u64, |k, &v| mix(k, v)),
            lines: &lines,
            marks,
            marks_key: lines_key,
        };
        draw(&frame, &overlay, text)
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
                    body: r.shader.as_ref()?.body.clone(),
                    constants: r.shader.as_ref()?.constants.clone(),
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
        "value": r.value.as_ref().map(|v| match &r.unit {
            Some(unit) => format!("{}\\ {unit}", result_latex(v)),
            None => result_latex(v),
        }),
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
        "points": !(r.points.is_empty() && r.points3.is_empty()),
        // Whether the row has a fill, for the Fill slider.
        "polygons": !(r.polygons.is_empty()
            && r.polygons3.is_empty()
            && r.boxes3.is_empty()
            && r.implicits3.iter().all(|s| s.shade.is_none())),
        "updates": r.updates.iter().filter_map(|(name, v)| Some(json!({
            "row": defined.get(name.as_str())?,
            "latex": exact_latex(v),
        }))).collect::<Vec<_>>(),
        "lines": !(r.curves.is_empty()
            && r.odes.is_empty()
            && r.parametrics.is_empty()
            && r.regions.is_empty()
            && r.curves3.is_empty()),
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
        // A differential equation shows its initial points under it.
        "ode": r.ode.then(|| json!({"error": r.seeds_error})),
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
    fn three_d_rows_become_surfaces_with_kept_meshes() {
        let mut engine = Engine {
            mode: Mode::ThreeD,
            ..Engine::default()
        };
        let reply = engine.update(&json!([
            {"id": 1, "latex": "z=x+y", "color": "#2d70b3"},
            {"id": 2, "latex": "y=x"},
            {"id": 3, "latex": "x+z"},
        ]));
        assert_eq!(engine.surfaces.len(), 2, "y = x is a wall in 3D");
        assert_eq!(engine.curves.len(), 0, "flat curves are not drawn in 3D");
        assert_eq!(reply[0]["graphed"], true);
        assert!(reply[2]["error"].is_string());
        let mut view = View3::new(400.0, 300.0);
        let mut text = TextAtlas::empty();
        let key = engine.with_frame_3d(&view, 1.0, &mut text, |f, _, _| {
            assert_eq!(f.surfaces.len(), 2);
            assert!(!f.lines.is_empty());
            f.surfaces_key
        });
        // Points and curves go into the frame's lines.
        let mut with_more = Engine {
            mode: Mode::ThreeD,
            ..Engine::default()
        };
        let reply = with_more.update(&json!([
            {"id": 1, "latex": "\\left(1,2,3\\right)", "pointSize": 12},
            {"id": 2, "latex": "\\left(\\cos t,\\sin t,t\\right)"},
        ]));
        assert_eq!(reply[0]["points"], true);
        assert_eq!(reply[1]["lines"], true);
        assert_eq!(with_more.points3[0].size, 12.0);
        assert_eq!(with_more.curves3.len(), 1);
        let bare = engine.with_frame_3d(&view, 1.0, &mut text, |f, _, _| f.marks.len());
        let (more, marks_key) = with_more.with_frame_3d(&view, 1.0, &mut text, |f, _, _| {
            (f.marks.len(), f.marks_key)
        });
        assert_eq!(bare, 0);
        assert!(more > 6, "{more} vertices for a point and a curve");
        // Points and curves are kept while the box turns.
        let mut turned = view;
        turned.turn_px(40.0, 0.0);
        let same = with_more.with_frame_3d(&turned, 1.0, &mut text, |f, _, _| f.marks_key);
        assert_eq!(same, marks_key);

        // Turning the box keeps the mesh; zooming makes a new one.
        view.turn_px(40.0, 0.0);
        let turned = engine.with_frame_3d(&view, 1.0, &mut text, |f, _, _| f.surfaces_key);
        assert_eq!(turned, key);
        view.zoom(2.0);
        let zoomed = engine.with_frame_3d(&view, 1.0, &mut text, |f, _, _| f.surfaces_key);
        assert_ne!(zoomed, key);
    }

    #[test]
    fn solids_have_a_fill_slider_that_reaches_solid() {
        let mut engine = Engine {
            mode: Mode::ThreeD,
            ..Engine::default()
        };
        let reply = engine.update(&json!([
            {"id": 1, "latex": "x^2+y^2+z^2<4", "color": "#2d70b3"},
            {"id": 2, "latex": "0\\le z\\le 1", "color": "#2d70b3", "fillOpacity": 1},
        ]));
        assert_eq!(reply[0]["polygons"], true, "a solid shows the Fill slider");
        assert_eq!(engine.surfaces[0].color[3], SOLID_OPACITY);
        assert_eq!(engine.surfaces[1].color[3], 1.0, "Fill at 1 is solid");
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
