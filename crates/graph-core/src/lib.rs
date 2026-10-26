//! Pure graphing math with no GPU or window dependencies, so it can be unit
//! tested anywhere and reused by every backend.

pub mod grid;
pub mod sample;
pub mod scene;
pub mod tess;
pub mod viewport;

pub use scene::{build_scene, Curve};
pub use tess::Vertex;
pub use viewport::Viewport;
