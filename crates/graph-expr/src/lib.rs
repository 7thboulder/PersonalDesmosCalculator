//! The expression engine: reads each row's LaTeX the way Desmos does,
//! resolves names across rows, and compiles curves into fast programs.

pub mod analyze;
pub mod ast;
pub mod builtins;
pub mod calculus;
pub mod compile;
mod fastsum;
pub mod format;
pub mod interval;
pub mod lexer;
pub mod parser;
pub mod wgsl;

pub use analyze::{
    analyze, analyze_cached, analyze_mode, Axis, Cache, ColumnAnalysis, ColumnInput, Compare,
    Curve, Curve3, Fit, Fractal, Implicit3, Mode, ParamSurface, Parameter, Parametric, Region,
    RowAnalysis, RowInput, RowKind, Surface, Value,
};
pub use compile::Program;
pub use interval::Interval;
pub use parser::parse;
