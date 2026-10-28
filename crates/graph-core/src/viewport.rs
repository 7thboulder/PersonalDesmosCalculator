/// The visible region of the graph paper.
///
/// Screen coordinates are physical pixels with the origin at the top-left of
/// the graph area and y growing downwards. World coordinates are math units
/// with y growing upwards. Both axes share one scale, like Desmos's default
/// square grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    /// World point shown at the centre of the graph area.
    pub center: [f64; 2],
    /// World units per physical pixel.
    pub units_per_px: f64,
    pub width_px: f64,
    pub height_px: f64,
}

/// About as fine as f64 can place a pixel near 1, which fractal rows zoom
/// all the way down to.
const MIN_UNITS_PER_PX: f64 = 1e-15;
const MAX_UNITS_PER_PX: f64 = 1e12;

impl Viewport {
    /// Desmos's starting view: x from -10 to 10, centred on the origin.
    pub fn new(width_px: f64, height_px: f64) -> Self {
        let width_px = width_px.max(1.0);
        Self {
            center: [0.0, 0.0],
            units_per_px: 20.0 / width_px,
            width_px,
            height_px: height_px.max(1.0),
        }
    }

    /// Changes the pixel size while keeping the centre and the scale, so
    /// resizing the window reveals more paper instead of stretching it.
    pub fn resize(&mut self, width_px: f64, height_px: f64) {
        self.width_px = width_px.max(1.0);
        self.height_px = height_px.max(1.0);
    }

    pub fn to_screen(&self, x: f64, y: f64) -> [f64; 2] {
        [
            (x - self.center[0]) / self.units_per_px + self.width_px / 2.0,
            self.height_px / 2.0 - (y - self.center[1]) / self.units_per_px,
        ]
    }

    pub fn to_world(&self, px: f64, py: f64) -> [f64; 2] {
        [
            self.center[0] + (px - self.width_px / 2.0) * self.units_per_px,
            self.center[1] - (py - self.height_px / 2.0) * self.units_per_px,
        ]
    }

    pub fn x_range(&self) -> (f64, f64) {
        (
            self.to_world(0.0, 0.0)[0],
            self.to_world(self.width_px, 0.0)[0],
        )
    }

    pub fn y_range(&self) -> (f64, f64) {
        (
            self.to_world(0.0, self.height_px)[1],
            self.to_world(0.0, 0.0)[1],
        )
    }

    /// Moves the paper with the mouse: dragging right shows what was on the left.
    pub fn pan_px(&mut self, dx: f64, dy: f64) {
        self.center[0] -= dx * self.units_per_px;
        self.center[1] += dy * self.units_per_px;
    }

    /// Zooms by `factor` (above 1 zooms in) keeping the world point under the
    /// cursor fixed, as Desmos does with the scroll wheel.
    pub fn zoom_at(&mut self, px: f64, py: f64, factor: f64) {
        let before = self.to_world(px, py);
        self.units_per_px = (self.units_per_px / factor).clamp(MIN_UNITS_PER_PX, MAX_UNITS_PER_PX);
        let after = self.to_world(px, py);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9 * (1.0 + a.abs().max(b.abs()))
    }

    #[test]
    fn default_view_spans_minus_ten_to_ten() {
        let vp = Viewport::new(1000.0, 600.0);
        let (x0, x1) = vp.x_range();
        assert!(close(x0, -10.0) && close(x1, 10.0));
        let (y0, y1) = vp.y_range();
        assert!(close(y0, -6.0) && close(y1, 6.0));
    }

    #[test]
    fn screen_and_world_round_trip() {
        let mut vp = Viewport::new(800.0, 500.0);
        vp.center = [3.5, -2.0];
        vp.units_per_px = 0.037;
        let [px, py] = vp.to_screen(1.25, 7.5);
        let [x, y] = vp.to_world(px, py);
        assert!(close(x, 1.25) && close(y, 7.5));
    }

    #[test]
    fn zoom_keeps_point_under_cursor() {
        let mut vp = Viewport::new(800.0, 500.0);
        let anchor = vp.to_world(123.0, 456.0);
        vp.zoom_at(123.0, 456.0, 1.7);
        let after = vp.to_world(123.0, 456.0);
        assert!(close(anchor[0], after[0]) && close(anchor[1], after[1]));
    }

    #[test]
    fn pan_moves_paper_with_drag() {
        let mut vp = Viewport::new(1000.0, 600.0);
        vp.pan_px(100.0, 50.0);
        // Dragging right by 100 px (2 units) moves the centre left by 2 units.
        assert!(close(vp.center[0], -2.0) && close(vp.center[1], 1.0));
    }
}
