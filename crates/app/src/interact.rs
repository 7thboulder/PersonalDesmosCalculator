//! Mouse interaction with the graph, kept free of window code so it is
//! tested on every platform: points of interest on the selected curve,
//! tracing along a curve, and dragging points whose coordinates are
//! sliders. Positions are physical pixels within the graph area.
// Only the Windows window drives this outside tests.
#![cfg_attr(not(windows), allow(dead_code))]

use crate::engine::Engine;
use graph_core::{curve_pois, intersections, point_text, Axis, Mark, MarkKind, Poi, Viewport};

/// How close, in logical pixels, the mouse has to be to grab something.
const GRAB_PX: f64 = 10.0;
/// Draggable points are a little easier to grab, as in Desmos.
const DRAG_GRAB_PX: f64 = 14.0;
/// A trace point jumps onto a point of interest this close to it.
const SNAP_PX: f64 = 8.0;

/// What the window should do after an event.
#[derive(Debug, PartialEq)]
pub enum Effect {
    /// Nothing changed.
    None,
    Redraw,
    /// The press is on empty paper: move the view with the mouse.
    Pan,
    /// Select this row in the panel.
    Select(u64),
    /// Nothing is selected any more.
    Deselect,
    /// Set slider rows to new values, as `(row, value)`.
    SetSliders(Vec<(u64, f64)>),
}

#[derive(Clone, Debug, PartialEq)]
enum Mode {
    Idle,
    Pan,
    /// Following the mouse along `engine.curves[curve]`.
    Trace { curve: usize },
    /// Moving a point; the slider rows its x and y set.
    Drag { sliders: [Option<u64>; 2] },
}

/// The selected row's points of interest, remembered until the view or the
/// rows change.
struct PoiCache {
    key: (u64, Option<u64>, [u64; 5]),
    pois: Vec<Poi>,
}

pub struct Interaction {
    mode: Mode,
    /// The mouse, when it is over the graph.
    hover: Option<[f64; 2]>,
    /// A point of interest whose label was clicked open.
    pinned: Option<[f64; 2]>,
    /// The trace point and its curve's colour, left showing after release.
    trace: Option<([f64; 2], [f32; 4])>,
    cache: Option<PoiCache>,
    /// The display's DPI scale.
    pub scale: f64,
}

impl Default for Interaction {
    fn default() -> Self {
        Self {
            mode: Mode::Idle,
            hover: None,
            pinned: None,
            trace: None,
            cache: None,
            scale: 1.0,
        }
    }
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

impl Interaction {
    /// The selected row's points of interest: each of its explicit curves'
    /// own, and where they meet the other drawn curves.
    pub fn pois(&mut self, engine: &Engine, vp: &Viewport) -> &[Poi] {
        let view = [
            vp.center[0].to_bits(),
            vp.center[1].to_bits(),
            vp.units_per_px.to_bits(),
            vp.width_px.to_bits(),
            vp.height_px.to_bits(),
        ];
        let key = (engine.generation, engine.selected, view);
        if self.cache.as_ref().map(|c| c.key) != Some(key) {
            let mut pois = Vec::new();
            if let Some(row) = engine.selected {
                for c in engine.curves.iter().filter(|c| c.row == row) {
                    let f = |t: f64| c.program.eval(t);
                    pois.extend(curve_pois(&f, vp, c.axis == Axis::Y));
                    if c.axis != Axis::X {
                        continue;
                    }
                    for other in engine.curves.iter().filter(|o| o.row != row && o.axis == Axis::X) {
                        let g = |t: f64| other.program.eval(t);
                        pois.extend(intersections(&f, &g, vp));
                    }
                }
            }
            self.cache = Some(PoiCache {
                key,
                pois: graph_core::poi::dedupe(pois, vp),
            });
        }
        &self.cache.as_ref().expect("filled above").pois
    }

    fn near_poi(&mut self, engine: &Engine, vp: &Viewport, at: [f64; 2], px: f64) -> Option<[f64; 2]> {
        let reach = px * self.scale;
        self.pois(engine, vp)
            .iter()
            .map(|p| (p.at, dist(vp.to_screen(p.at[0], p.at[1]), at)))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(p, _)| p)
    }

    /// The nearest drawn explicit curve within reach of `at`, and the point
    /// on it under the mouse.
    fn near_curve(&self, engine: &Engine, vp: &Viewport, at: [f64; 2]) -> Option<(usize, [f64; 2])> {
        let reach = GRAB_PX * self.scale;
        engine
            .curves
            .iter()
            .enumerate()
            .filter_map(|(i, _)| {
                let p = curve_point(engine, i, vp, at)?;
                let d = dist(vp.to_screen(p[0], p[1]), at);
                (d <= reach).then_some((i, p, d))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(i, p, _)| (i, p))
    }

    /// A mouse press at `at`.
    pub fn press(&mut self, engine: &Engine, vp: &Viewport, at: [f64; 2]) -> Effect {
        self.hover = Some(at);
        // Draggable points come first, then points of interest, then curves.
        let reach = DRAG_GRAB_PX * self.scale;
        let grabbed = engine
            .points
            .iter()
            .filter(|p| p.drag.is_some() && p.points.len() == 1)
            .map(|p| (p, dist(vp.to_screen(p.points[0][0], p.points[0][1]), at)))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((p, _)) = grabbed {
            let drag = p.drag.as_ref().expect("filtered");
            let slider = |n: &Option<String>| n.as_ref().and_then(|n| engine.sliders.get(n).copied());
            self.mode = Mode::Drag {
                sliders: [slider(&drag[0]), slider(&drag[1])],
            };
            self.trace = None;
            return Effect::Select(p.row);
        }
        if let Some(poi) = self.near_poi(engine, vp, at, GRAB_PX) {
            self.pinned = Some(poi);
            self.trace = None;
            self.mode = Mode::Pan;
            return Effect::Redraw;
        }
        if let Some((curve, p)) = self.near_curve(engine, vp, at) {
            self.mode = Mode::Trace { curve };
            self.pinned = None;
            let c = &engine.curves[curve];
            self.trace = Some((p, c.color));
            return Effect::Select(c.row);
        }
        self.mode = Mode::Pan;
        let had = self.pinned.take().is_some() | self.trace.take().is_some();
        if engine.selected.is_some() {
            Effect::Deselect
        } else if had {
            Effect::Redraw
        } else {
            Effect::Pan
        }
    }

    /// The mouse moved to `at`, with or without a button held.
    pub fn moved(&mut self, engine: &Engine, vp: &Viewport, at: [f64; 2]) -> Effect {
        let before = self.hover_poi(engine, vp);
        self.hover = Some(at);
        match self.mode.clone() {
            Mode::Pan => Effect::Pan,
            Mode::Trace { curve } => {
                let Some(mut p) = curve_point(engine, curve, vp, at) else {
                    return Effect::None;
                };
                let s = vp.to_screen(p[0], p[1]);
                if let Some(poi) = self.near_poi(engine, vp, s, SNAP_PX) {
                    p = poi;
                }
                self.trace = Some((p, engine.curves[curve].color));
                Effect::Redraw
            }
            Mode::Drag { sliders } => {
                let w = vp.to_world(at[0], at[1]);
                // About a pixel's precision, as Desmos rounds dragged points.
                let places = ((-vp.units_per_px.log10()).ceil()).clamp(0.0, 12.0) as i32;
                let round = |v: f64| {
                    let k = 10f64.powi(places);
                    (v * k).round() / k
                };
                let set: Vec<(u64, f64)> = sliders
                    .iter()
                    .zip(w)
                    .filter_map(|(s, v)| s.map(|row| (row, round(v))))
                    .collect();
                if set.is_empty() {
                    Effect::None
                } else {
                    Effect::SetSliders(set)
                }
            }
            Mode::Idle => {
                if self.hover_poi(engine, vp) != before {
                    Effect::Redraw
                } else {
                    Effect::None
                }
            }
        }
    }

    pub fn release(&mut self) {
        self.mode = Mode::Idle;
    }

    pub fn left(&mut self) -> Effect {
        self.hover = None;
        Effect::Redraw
    }

    /// Whether a press is moving the view.
    #[cfg(test)]
    pub fn panning(&self) -> bool {
        self.mode == Mode::Pan
    }

    /// Forget trace and labels when the selection moves to another row.
    pub fn selection_changed(&mut self) {
        self.pinned = None;
        if !matches!(self.mode, Mode::Trace { .. }) {
            self.trace = None;
        }
    }

    fn hover_poi(&mut self, engine: &Engine, vp: &Viewport) -> Option<[f64; 2]> {
        let at = self.hover?;
        self.near_poi(engine, vp, at, GRAB_PX)
    }

    /// Everything to draw on top of the graph.
    pub fn marks(&mut self, engine: &Engine, vp: &Viewport) -> Vec<Mark> {
        let hot = self.hover_poi(engine, vp);
        let pinned = self.pinned;
        let label = |p: [f64; 2]| Some(point_text(p, vp.units_per_px));
        let mut marks: Vec<Mark> = self
            .pois(engine, vp)
            .iter()
            .map(|p| {
                let lit = Some(p.at) == hot || Some(p.at) == pinned;
                Mark {
                    at: p.at,
                    color: [0.5, 0.5, 0.5, 1.0],
                    kind: if lit { MarkKind::Hot } else { MarkKind::Poi },
                    label: if lit { label(p.at) } else { None },
                }
            })
            .collect();
        if let Some((p, color)) = self.trace {
            marks.push(Mark {
                at: p,
                color,
                kind: MarkKind::Trace,
                label: label(p),
            });
        }
        marks
    }
}

/// The point on curve `i` under the mouse: straight above or below it for
/// y = f(x), level with it for x = g(y).
fn curve_point(engine: &Engine, i: usize, vp: &Viewport, at: [f64; 2]) -> Option<[f64; 2]> {
    let c = engine.curves.get(i)?;
    let w = vp.to_world(at[0], at[1]);
    let p = match c.axis {
        Axis::X => [w[0], c.program.eval(w[0])],
        Axis::Y => [c.program.eval(w[1]), w[1]],
    };
    (p[0].is_finite() && p[1].is_finite()).then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn setup(rows: serde_json::Value) -> (Engine, Viewport) {
        let mut engine = Engine::default();
        engine.update(&rows);
        (engine, Viewport::new(1000.0, 800.0))
    }

    #[test]
    fn selected_curve_shows_labelled_points_of_interest() {
        let (mut engine, vp) = setup(json!([
            {"id": 1, "latex": "y=x^2-4"},
            {"id": 2, "latex": "y=x-2"},
        ]));
        let mut ui = Interaction::default();
        assert!(ui.marks(&engine, &vp).is_empty(), "nothing selected");
        engine.selected = Some(1);
        let marks = ui.marks(&engine, &vp);
        // Zeros at ±2 (one shared with the line), the vertex, and where
        // the curves meet at x = -1.
        let has = |x: f64, y: f64| marks.iter().any(|m| dist(m.at, [x, y]) < 1e-9);
        assert!(has(2.0, 0.0) && has(-2.0, 0.0) && has(0.0, -4.0) && has(-1.0, -3.0), "{marks:?}");

        // Hovering one lights it up with Desmos's label.
        let at = vp.to_screen(0.0, -4.0);
        assert_eq!(ui.moved(&engine, &vp, [at[0] + 3.0, at[1]]), Effect::Redraw);
        let marks = ui.marks(&engine, &vp);
        let hot: Vec<_> = marks.iter().filter(|m| m.kind == MarkKind::Hot).collect();
        assert_eq!(hot.len(), 1);
        assert_eq!(hot[0].label.as_deref(), Some("(0, \u{2212}4)"));
    }

    #[test]
    fn pressing_a_curve_traces_it_and_empty_paper_pans() {
        let (engine, vp) = setup(json!([{"id": 7, "latex": "y=\\sin\\left(x\\right)"}]));
        let mut ui = Interaction::default();
        let on = vp.to_screen(1.0, 1f64.sin());
        assert_eq!(ui.press(&engine, &vp, [on[0], on[1] + 4.0]), Effect::Select(7));
        let to = vp.to_screen(2.0, 0.0);
        assert_eq!(ui.moved(&engine, &vp, to), Effect::Redraw);
        let trace = ui.marks(&engine, &vp).into_iter().find(|m| m.kind == MarkKind::Trace);
        let trace = trace.expect("a trace point");
        assert!((trace.at[0] - 2.0).abs() < 0.03 && (trace.at[1] - trace.at[0].sin()).abs() < 1e-12);
        ui.release();

        let far = vp.to_screen(3.0, 8.0);
        assert_eq!(ui.press(&engine, &vp, far), Effect::Redraw, "clears the trace");
        assert!(ui.panning());
        assert_eq!(ui.moved(&engine, &vp, [far[0] + 5.0, far[1]]), Effect::Pan);
    }

    #[test]
    fn dragging_a_slider_point_sets_its_sliders() {
        let (engine, vp) = setup(json!([
            {"id": 1, "latex": "a=1"},
            {"id": 2, "latex": "b=2"},
            {"id": 3, "latex": "\\left(a,b\\right)"},
        ]));
        let mut ui = Interaction::default();
        let at = vp.to_screen(1.0, 2.0);
        assert_eq!(ui.press(&engine, &vp, at), Effect::Select(3));
        let to = vp.to_screen(3.004, -1.5);
        assert_eq!(
            ui.moved(&engine, &vp, to),
            Effect::SetSliders(vec![(1, 3.0), (2, -1.5)])
        );
    }
}
