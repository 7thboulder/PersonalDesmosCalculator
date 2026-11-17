/// The graph settings that change how the paper is drawn, as in Desmos's
/// wrench menu.
#[derive(Clone, Debug, PartialEq)]
pub struct GridSettings {
    /// Circles and spokes about the origin instead of squares.
    pub polar: bool,
    /// Whether any grid lines show.
    pub grid: bool,
    /// The fainter lines between the major ones.
    pub minor: bool,
    pub x_axis: bool,
    pub y_axis: bool,
    pub x_numbers: bool,
    pub y_numbers: bool,
    /// Angles in degrees: trig functions take degrees, and the polar grid's
    /// spokes are labelled 30°, 60°, … instead of π/6, π/3, …
    pub degrees: bool,
    /// The axis steps and the normal distribution helper, as typed.
    pub axes: AxisText,
    /// Where random draws start from; re-randomizing changes it.
    pub random_seed: u64,
}

/// The wrench menu's axis steps, as typed: LaTeX that may name slider
/// variables, worked out by the engine on every update. Empty means the
/// zoom picks, as usual.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AxisText {
    /// The distance between major lines across.
    pub x_step: String,
    /// The distance between major lines up.
    pub y_step: String,
    /// Where the lines across are counted from, instead of 0.
    pub x_center: String,
    pub normal: NormalText,
}

/// The normal distribution helper: a mean and standard deviation that set
/// the x axis's centre and step. Its fields are kept while it is off.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalText {
    pub on: bool,
    /// μ and σ, as LaTeX.
    pub mean: String,
    pub sd: String,
    /// Label the x axis μ, μ+σ, μ−σ, … instead of numbers.
    pub sigma_labels: bool,
    /// Fit the y step to the peak, 1/(σ√(2π)), instead of `y_step`.
    pub y_auto: bool,
    /// Shade under the curve between two σ marks.
    pub shade: bool,
    pub shade_from: String,
    pub shade_to: String,
}

impl Default for NormalText {
    fn default() -> Self {
        Self {
            on: false,
            mean: "0".into(),
            sd: "1".into(),
            sigma_labels: true,
            y_auto: true,
            shade: false,
            shade_from: "-1".into(),
            shade_to: "1".into(),
        }
    }
}

/// The axis steps worked out to numbers, for drawing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AxisSteps {
    pub x_step: Option<f64>,
    pub x_center: f64,
    pub y_step: Option<f64>,
    /// μ and σ when the x axis is labelled in σs from μ.
    pub sigma: Option<[f64; 2]>,
    /// Where the y axis is drawn across: 0, or μ for the normal helper so
    /// the axis sits on the line at the mean rather than between two σ
    /// lines.
    pub y_axis_x: f64,
}

impl AxisSteps {
    /// How many times more units a pixel spans up than across: with both
    /// steps set, each grid cell is one x step wide and one y step tall,
    /// and square on screen.
    pub fn y_scale(&self) -> f64 {
        match (self.x_step, self.y_step) {
            (Some(x), Some(y)) => y / x,
            _ => 1.0,
        }
    }
}

/// Where one axis's lines go.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisLines {
    pub major: f64,
    pub minor: f64,
    /// The value lines are counted from: major line `i` is at
    /// `origin + i * major`.
    pub origin: f64,
    /// How many of a set step each major line is apart, when the step
    /// alone would crowd the lines together.
    pub multiple: i64,
    /// Whether the step was set rather than picked by the zoom.
    pub custom: bool,
}

/// One axis's lines: the zoom's own spacing, or `step` from `origin`, with
/// lines skipped by 2, 5, 10, … when they would come closer than about
/// two fifths of `target_major_px`.
pub fn axis_lines(
    step: Option<f64>,
    origin: f64,
    units_per_px: f64,
    target_major_px: f64,
) -> AxisLines {
    let origin = if origin.is_finite() { origin } else { 0.0 };
    let Some(step) = step.map(f64::abs).filter(|s| s.is_finite() && *s > 0.0) else {
        let auto = grid_spacing(units_per_px, target_major_px);
        return AxisLines {
            major: auto.major,
            minor: auto.minor,
            origin,
            multiple: 1,
            custom: false,
        };
    };
    let least = units_per_px * target_major_px * 0.4;
    let mut multiple: i64 = 1;
    let mut base: i64 = 1;
    'grow: while step * (multiple as f64) < least && multiple < 1 << 50 {
        for m in [1, 2, 5] {
            if m * base > multiple {
                multiple = m * base;
                continue 'grow;
            }
        }
        base *= 10;
    }
    let divisions = match multiple / base.max(1) {
        2 => 4.0,
        _ => 5.0,
    };
    let major = step * multiple as f64;
    AxisLines {
        major,
        minor: major / divisions,
        origin,
        multiple,
        custom: true,
    }
}

impl Default for GridSettings {
    fn default() -> Self {
        Self {
            polar: false,
            grid: true,
            minor: true,
            x_axis: true,
            y_axis: true,
            x_numbers: true,
            y_numbers: true,
            degrees: false,
            axes: AxisText::default(),
            random_seed: 0,
        }
    }
}

/// Spacing of the major and minor grid lines in world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridSpacing {
    pub major: f64,
    pub minor: f64,
}

/// Picks a 1-2-5 major spacing closest to `target_major_px` on screen, with
/// Desmos's subdivisions: 5 minor steps for 1 and 5, 4 minor steps for 2.
pub fn grid_spacing(units_per_px: f64, target_major_px: f64) -> GridSpacing {
    let target = units_per_px * target_major_px;
    let base = 10f64.powf(target.log10().floor());
    let candidates = [(1.0, 5.0), (2.0, 4.0), (5.0, 5.0), (10.0, 5.0)];
    let (mantissa, divisions) = candidates
        .into_iter()
        .min_by(|a, b| {
            let da = (base * a.0 / target).ln().abs();
            let db = (base * b.0 / target).ln().abs();
            da.total_cmp(&db)
        })
        .unwrap();
    let major = base * mantissa;
    GridSpacing {
        major,
        minor: major / divisions,
    }
}

/// Multiples of `step` inside `[min, max]`, returned as integer indices so
/// callers can tell major lines from minor ones without float comparisons.
pub fn line_indices(min: f64, max: f64, step: f64) -> std::ops::RangeInclusive<i64> {
    let first = (min / step).ceil() as i64;
    let last = (max / step).floor() as i64;
    first..=last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_view_matches_desmos_two_unit_grid() {
        // 20 units across 1000 px, aiming for ~100 px between major lines.
        let g = grid_spacing(0.02, 100.0);
        assert!((g.major - 2.0).abs() < 1e-12);
        assert!((g.minor - 0.5).abs() < 1e-12);
    }

    #[test]
    fn spacing_follows_one_two_five() {
        for (upp, major) in [(0.01, 1.0), (0.04, 5.0), (0.0004, 0.05), (90.0, 10000.0)] {
            let g = grid_spacing(upp, 100.0);
            assert!((g.major / major - 1.0).abs() < 1e-9, "{upp} -> {}", g.major);
        }
    }

    #[test]
    fn set_steps_thin_out_when_crowded() {
        // σ = 1 at 0.02 units a pixel is 50 px apart: every one shows.
        let a = axis_lines(Some(1.0), 70.0, 0.02, 100.0);
        assert_eq!((a.major, a.multiple, a.origin), (1.0, 1, 70.0));
        // Zoomed out 10×, every 5th σ.
        let a = axis_lines(Some(1.0), 0.0, 0.2, 100.0);
        assert_eq!((a.major, a.multiple), (10.0, 10));
        let a = axis_lines(Some(1.0), 0.0, 0.05, 100.0);
        assert_eq!((a.major, a.multiple), (2.0, 2));
        assert!((a.minor - 0.5).abs() < 1e-12);
        // No step keeps the zoom's own.
        let a = axis_lines(None, 0.0, 0.02, 100.0);
        assert!(!a.custom && (a.major - 2.0).abs() < 1e-12);
    }

    #[test]
    fn both_steps_make_square_cells() {
        let s = AxisSteps {
            x_step: Some(10.0),
            y_step: Some(0.01),
            ..Default::default()
        };
        assert!((s.y_scale() - 0.001).abs() < 1e-15);
        assert_eq!(AxisSteps::default().y_scale(), 1.0);
    }

    #[test]
    fn indices_cover_range() {
        assert_eq!(line_indices(-2.5, 3.1, 1.0), -2..=3);
    }
}
