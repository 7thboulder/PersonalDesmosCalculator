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
    fn indices_cover_range() {
        assert_eq!(line_indices(-2.5, 3.1, 1.0), -2..=3);
    }
}
