//! The WGSL helpers that generated shaders call.
//!
//! Relations are turned into WGSL by graph-expr and pasted into a fragment
//! shader by graph-render; both sides agree on these helpers, which follow
//! the same numeric rules as the CPU evaluator, including Desmos's real
//! powers of negative bases.

/// Helper functions every generated shader includes once.
pub const PRELUDE: &str = r#"
fn gc_mod(x: f32, b: f32) -> f32 { return x - b * floor(x / b); }

fn gc_pow(x: f32, y: f32) -> f32 {
    if (x >= 0.0) { return pow(x, y); }
    let r = round(y);
    if (abs(y - r) < 1e-6) {
        let m = pow(-x, y);
        if (abs(gc_mod(r, 2.0) - 1.0) < 0.5) { return -m; }
        return m;
    }
    let inv = 1.0 / y;
    let ri = round(inv);
    if (abs(inv - ri) < 1e-4 && abs(gc_mod(ri, 2.0) - 1.0) < 0.5) {
        return -pow(-x, y);
    }
    return gc_nan();
}

fn gc_nan() -> f32 { return bitcast<f32>(0x7fc00000u); }

// Comparisons are 1 or 0, and undefined when either side is.
fn gc_truth(a: f32, b: f32, holds: bool) -> f32 {
    if (a != a || b != b) { return gc_nan(); }
    return select(0.0, 1.0, holds);
}

fn gc_select(c: f32, t: f32, e: f32) -> f32 {
    if (c != c) { return gc_nan(); }
    return select(e, t, c != 0.0);
}

fn gc_sign(x: f32) -> f32 { return sign(x); }
fn gc_cot(x: f32) -> f32 { return cos(x) / sin(x); }
fn gc_coth(x: f32) -> f32 { return cosh(x) / sinh(x); }
fn gc_round_to(x: f32, places: f32) -> f32 {
    let s = pow(10.0, round(places));
    return round(x * s) / s;
}
"#;
