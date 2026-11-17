//! The wrench menu's axis steps and its normal distribution helper. What
//! the person typed may name slider variables, so each field is read as an
//! extra row next to the panel's on every update, and the grid follows the
//! sliders as they move.

use graph_core::scene::sigma_label;
use graph_core::{AxisSteps, AxisText};
use graph_expr::special::erf;
use graph_expr::{RowAnalysis, RowInput};
use serde_json::{json, Map, Value};

/// The ids of the extra rows, far above any the panel hands out.
const PROBE_ID: u64 = u64::MAX - 64;

/// The y step "auto" picks: the peak sits on the fourth line up.
pub const PEAK_LINES: f64 = 4.0;

/// How many points trace the top of a shaded stretch.
const SHADE_POINTS: usize = 240;

/// The fields that can hold an expression, as the menu names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    XStep,
    YStep,
    XCenter,
    Mean,
    Sd,
    ShadeFrom,
    ShadeTo,
}

impl Field {
    const ALL: [Field; 7] = [
        Field::XStep,
        Field::YStep,
        Field::XCenter,
        Field::Mean,
        Field::Sd,
        Field::ShadeFrom,
        Field::ShadeTo,
    ];

    fn name(self) -> &'static str {
        match self {
            Field::XStep => "xStep",
            Field::YStep => "yStep",
            Field::XCenter => "xCenter",
            Field::Mean => "mean",
            Field::Sd => "sd",
            Field::ShadeFrom => "shadeFrom",
            Field::ShadeTo => "shadeTo",
        }
    }

    fn text(self, t: &AxisText) -> &str {
        match self {
            Field::XStep => &t.x_step,
            Field::YStep => &t.y_step,
            Field::XCenter => &t.x_center,
            Field::Mean => &t.normal.mean,
            Field::Sd => &t.normal.sd,
            Field::ShadeFrom => &t.normal.shade_from,
            Field::ShadeTo => &t.normal.shade_to,
        }
    }
}

/// The extra rows to analyze after the panel's: one for each filled field.
/// Wrapped in brackets, so `a=3` typed into a field cannot define `a`.
pub fn probes(text: &AxisText) -> Vec<RowInput> {
    Field::ALL
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.text(text).trim().is_empty())
        .map(|(i, f)| RowInput {
            id: PROBE_ID + i as u64,
            latex: format!("\\left({}\\right)", f.text(text)),
            ..Default::default()
        })
        .collect()
}

/// The shaded part of a normal curve between two σ marks.
#[derive(Clone, Debug, PartialEq)]
pub struct Shade {
    /// The outline: along the curve, then back along y = 0.
    pub outline: Vec<[f64; 2]>,
    /// Where the probability is written, and what it says.
    pub note_at: [f64; 2],
    pub note: String,
}

/// The axis steps worked out to numbers.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resolved {
    pub steps: AxisSteps,
    /// μ and σ, while the helper is on and both are numbers (σ > 0).
    pub normal: Option<[f64; 2]>,
    pub shade: Option<Shade>,
    /// What the menu shows: each field's value, which fields are not
    /// numbers, the peak and the shaded probability.
    pub json: Value,
}

/// The normal density.
pub fn pdf(x: f64, mean: f64, sd: f64) -> f64 {
    let z = (x - mean) / sd;
    (-0.5 * z * z).exp() / (sd * (2.0 * std::f64::consts::PI).sqrt())
}

/// The probability of falling within `a` to `b` σs of the mean.
pub fn probability(a: f64, b: f64) -> f64 {
    let cdf = |k: f64| 0.5 * (1.0 + erf(k / std::f64::consts::SQRT_2));
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    cdf(hi) - cdf(lo)
}

/// `μ+1.5σ`, `μ−σ`, `−∞` for a mark `k` σs from the mean.
fn mark_text(k: f64) -> String {
    if k.is_infinite() {
        return if k > 0.0 {
            "\u{221e}"
        } else {
            "\u{2212}\u{221e}"
        }
        .to_string();
    }
    if k == k.round() && k.abs() < 1e6 {
        return sigma_label(k as i64);
    }
    let digits = format!("{:.3}", k.abs());
    let digits = digits.trim_end_matches('0').trim_end_matches('.');
    let sign = if k < 0.0 { '\u{2212}' } else { '+' };
    format!("\u{3bc}{sign}{digits}\u{3c3}")
}

/// Reads the extra rows' values (in `results`, among the panel's) into
/// numbers for the grid, the shading and the menu.
pub fn resolve(text: &AxisText, results: &[RowAnalysis]) -> Resolved {
    let mut values = Map::new();
    let mut errors = Vec::new();
    let value = |f: Field| -> Option<f64> {
        let i = Field::ALL.iter().position(|g| *g == f)? as u64;
        results
            .iter()
            .find(|r| r.id == PROBE_ID + i)
            .and_then(RowAnalysis::number)
            .filter(|v| !v.is_nan())
    };
    for f in Field::ALL {
        if f.text(text).trim().is_empty() {
            continue;
        }
        match value(f) {
            Some(v) => {
                values.insert(f.name().into(), json!(v.is_finite().then_some(v)));
            }
            None => errors.push(f.name()),
        }
    }
    let positive = |v: Option<f64>| v.map(f64::abs).filter(|v| v.is_finite() && *v > 0.0);
    let n = &text.normal;
    let normal = match (value(Field::Mean), positive(value(Field::Sd))) {
        (Some(mean), Some(sd)) if n.on && mean.is_finite() => Some([mean, sd]),
        _ => None,
    };
    if n.on && normal.is_none() && value(Field::Sd).is_some() && !errors.contains(&"sd") {
        errors.push("sd"); // σ must be above 0.
    }
    let mut steps = AxisSteps {
        x_step: positive(value(Field::XStep)),
        x_center: value(Field::XCenter)
            .filter(|v| v.is_finite())
            .unwrap_or(0.0),
        y_step: positive(value(Field::YStep)),
        sigma: None,
        y_axis_x: 0.0,
    };
    let mut reply = json!({ "values": values, "errors": errors });
    let mut shade = None;
    if let Some([mean, sd]) = normal {
        let peak = pdf(mean, mean, sd);
        steps.x_step = Some(sd);
        steps.x_center = mean;
        // The y axis stands on the line at the mean, so it lines up with
        // the σ marks instead of falling between two of them.
        steps.y_axis_x = mean;
        if n.y_auto {
            steps.y_step = Some(peak / PEAK_LINES);
        }
        if n.sigma_labels {
            steps.sigma = Some([mean, sd]);
        }
        reply["peak"] = json!(peak);
        if n.shade {
            if let (Some(a), Some(b)) = (value(Field::ShadeFrom), value(Field::ShadeTo)) {
                let p = probability(a, b);
                reply["probability"] = json!(p);
                let mut s = shade_between(mean, sd, a.min(b), a.max(b), p);
                // Over the mean the note would sit on the y axis numbers,
                // so it moves to halfway between two y grid lines.
                if let Some(step) = steps.y_step.filter(|_| s.note_at[0] == mean) {
                    let y = ((s.note_at[1] / step).floor() + 0.5) * step;
                    s.note_at[1] = y.min(peak * 0.9);
                }
                shade = Some(s);
            }
        }
    }
    Resolved {
        steps,
        normal,
        shade,
        json: reply,
    }
}

/// The region under the curve from `a` to `b` σs, with its probability
/// written in the middle of it.
fn shade_between(mean: f64, sd: f64, a: f64, b: f64, p: f64) -> Shade {
    // Past 8σ the curve is flat on the axis.
    let (lo, hi) = (a.clamp(-8.0, 8.0), b.clamp(-8.0, 8.0));
    let mut outline: Vec<[f64; 2]> = (0..=SHADE_POINTS)
        .map(|i| {
            let k = lo + (hi - lo) * i as f64 / SHADE_POINTS as f64;
            let x = mean + k * sd;
            [x, pdf(x, mean, sd)]
        })
        .collect();
    outline.push([mean + hi * sd, 0.0]);
    outline.push([mean + lo * sd, 0.0]);
    // The note sits a third of the way up the curve over the middle of
    // the stretch, or over the peak when the stretch takes it in.
    let mid = if lo <= 0.0 && 0.0 <= hi {
        0.0
    } else {
        (lo + hi) / 2.0
    };
    let x = mean + mid * sd;
    let note = format!(
        "P({} < x < {}) = {}",
        mark_text(a),
        mark_text(b),
        trim(format!("{p:.4}"))
    );
    Shade {
        outline,
        note_at: [x, pdf(x, mean, sd) * 0.35],
        note,
    }
}

fn trim(s: String) -> String {
    let t = s.trim_end_matches('0').trim_end_matches('.');
    if t.is_empty() {
        "0".into()
    } else {
        t.into()
    }
}

/// The view that shows the whole bell: μ ± 4.5σ across and from just below
/// 0 to above the peak, given the graph's size in pixels and how many
/// times more units a pixel spans up than across. Returns the centre and
/// the units per pixel across.
pub fn fit([mean, sd]: [f64; 2], y_scale: f64, width: f64, height: f64) -> ([f64; 2], f64) {
    let (width, height) = (width.max(1.0), height.max(1.0));
    let peak = pdf(mean, mean, sd);
    let across = 9.0 * sd;
    // Up: a tenth of the peak below 0 to a quarter above the peak.
    let up = peak * 1.35;
    let units = (across / width).max(up / (height * y_scale));
    ([mean, peak * (1.25 - 0.1) / 2.0], units)
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph_core::NormalText;
    use graph_expr::{analyze_angles, Cache, Mode};

    fn run(rows: &[&str], text: &AxisText) -> Resolved {
        let mut inputs: Vec<RowInput> = rows
            .iter()
            .enumerate()
            .map(|(i, l)| RowInput {
                id: i as u64 + 1,
                latex: l.to_string(),
                ..Default::default()
            })
            .collect();
        inputs.extend(probes(text));
        let results = analyze_angles(&inputs, &mut Cache::default(), Mode::Flat, false);
        resolve(text, &results)
    }

    fn helper(mean: &str, sd: &str) -> AxisText {
        AxisText {
            normal: NormalText {
                on: true,
                mean: mean.into(),
                sd: sd.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn steps_read_slider_variables() {
        let text = AxisText {
            x_step: "2s".into(),
            y_step: "0.5".into(),
            x_center: "m".into(),
            ..Default::default()
        };
        let r = run(&["m=70", "s=10"], &text);
        assert_eq!(r.steps.x_step, Some(20.0));
        assert_eq!(r.steps.y_step, Some(0.5));
        assert_eq!(r.steps.x_center, 70.0);
        assert!((r.steps.y_scale() - 0.025).abs() < 1e-12);
    }

    #[test]
    fn helper_centres_on_the_mean_in_sigma_steps() {
        let r = run(&["m=100", "s=15"], &helper("m", "s"));
        assert_eq!(r.normal, Some([100.0, 15.0]));
        assert_eq!(r.steps.x_step, Some(15.0));
        assert_eq!(r.steps.x_center, 100.0);
        assert_eq!(r.steps.sigma, Some([100.0, 15.0]));
        // Auto: the peak, 1/(σ√(2π)), is four lines up.
        let peak = 1.0 / (15.0 * (2.0 * std::f64::consts::PI).sqrt());
        assert!((r.steps.y_step.unwrap() * 4.0 - peak).abs() < 1e-15);
        // The y axis stands on the mean's line.
        assert_eq!(r.steps.y_axis_x, 100.0);
        assert!((r.json["peak"].as_f64().unwrap() - peak).abs() < 1e-15);
    }

    #[test]
    fn bad_fields_are_named() {
        let r = run(&[], &helper("q", "-0"));
        let errors = r.json["errors"].as_array().unwrap();
        assert!(errors.contains(&json!("mean")) && errors.contains(&json!("sd")));
        assert_eq!(r.normal, None);
        assert_eq!(r.steps, AxisSteps::default());
    }

    #[test]
    fn shading_one_sigma_each_way_is_68_percent() {
        let mut text = helper("0", "1");
        text.normal.shade = true;
        let r = run(&[], &text);
        let p = r.json["probability"].as_f64().unwrap();
        assert!((p - 0.682_689_492).abs() < 1e-6, "{p}");
        let shade = r.shade.unwrap();
        assert_eq!(
            shade.note,
            "P(\u{3bc}\u{2212}\u{3c3} < x < \u{3bc}+\u{3c3}) = 0.6827"
        );
        assert_eq!(shade.outline.first().unwrap()[0], -1.0);
        assert_eq!(shade.outline.last().unwrap(), &[-1.0, 0.0]);
        // To infinity: everything above the mean is a half.
        text.normal.shade_from = "0".into();
        text.normal.shade_to = "\\infty".into();
        let r = run(&[], &text);
        assert!((r.json["probability"].as_f64().unwrap() - 0.5).abs() < 1e-9);
        assert!(r.shade.unwrap().note.ends_with("\u{221e}) = 0.5"));
    }

    #[test]
    fn the_extra_rows_stay_out_of_the_panel() {
        let mut engine = crate::engine::Engine::default();
        engine.grid.axes = helper("m", "s");
        engine.grid.axes.normal.shade = true;
        let rows = serde_json::json!([
            {"id": 1, "latex": "m=70"},
            {"id": 2, "latex": "s=10"},
        ]);
        let reply = engine.update(&rows);
        assert_eq!(reply.as_array().unwrap().len(), 2);
        assert_eq!(engine.latex.len(), 2);
        assert_eq!(engine.axes.normal, Some([70.0, 10.0]));
        // Dragging s to 5 halves the step.
        let rows = serde_json::json!([
            {"id": 1, "latex": "m=70"},
            {"id": 2, "latex": "s=5"},
        ]);
        engine.update(&rows);
        assert_eq!(engine.axes.steps.x_step, Some(5.0));
        assert!(engine.axes.shade.is_some());
    }

    #[test]
    fn fit_shows_the_whole_bell() {
        let (mean, sd) = (70.0, 10.0);
        let peak = pdf(mean, mean, sd);
        let y_scale = (peak / PEAK_LINES) / sd;
        let (center, units) = fit([mean, sd], y_scale, 1000.0, 600.0);
        let half_w = 500.0 * units;
        let half_h = 300.0 * units * y_scale;
        assert!(center[0] - half_w <= mean - 4.0 * sd && center[0] + half_w >= mean + 4.0 * sd);
        assert!(center[1] - half_h < 0.0 && center[1] + half_h > peak);
    }
}
