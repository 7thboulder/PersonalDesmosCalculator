//! The expression engine: reads each row's LaTeX the way Desmos does,
//! resolves names across rows, and compiles curves into fast programs.

pub mod analyze;
pub mod ast;
pub mod builtins;
pub mod calculus;
pub mod compile;
pub mod format;
pub mod interval;
pub mod lexer;
pub mod parser;
pub mod wgsl;

pub use analyze::{
    analyze, analyze_cached, Axis, Cache, ColumnAnalysis, ColumnInput, Compare, Curve, Fit,
    Parameter, Parametric, Region, RowAnalysis, RowInput, RowKind, Value,
};
pub use compile::Program;
pub use interval::Interval;
pub use parser::parse;
