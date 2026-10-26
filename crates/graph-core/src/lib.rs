//! Pure graphing math with no GPU or window dependencies, so it can be unit
//! tested anywhere and reused by every backend.

pub mod grid;
pub mod implicit;
pub mod poi;
pub mod sample;
pub mod scene;
pub mod shader;
pub mod tess;
pub mod text;
pub mod viewport;

pub use implicit::{trace_implicit, Field};
pub use poi::{curve_pois, intersections, point_text, Poi, PoiKind};
pub use sample::{sample_parametric, Axis};
pub use scene::{
    build_scene, Curve, Implicit, Mark, MarkKind, Parametric, Plot, PointStyle, Points, Scene,
};
pub use tess::{LineStyle, Vertex};
pub use text::TextAtlas;
pub use viewport::Viewport;
