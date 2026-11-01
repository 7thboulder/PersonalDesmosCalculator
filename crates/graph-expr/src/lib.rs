//! The expression engine: reads each row's LaTeX the way Desmos does,
//! resolves names across rows, and compiles curves into fast programs.

pub mod analyze;
pub mod ast;
pub mod builtins;
pub mod calculus;
pub mod cas;
pub mod compile;
pub mod dots;
mod fastsum;
pub mod format;
pub mod interval;
pub mod lexer;
pub mod matrix;
pub mod parser;
pub mod special;
pub mod symbolic;
pub mod units;
pub mod wgsl;

pub use analyze::{
    analyze, analyze_cached, analyze_mode, Axis, Cache, ColumnAnalysis, ColumnInput, Compare,
    ComplexPlot, Curve, Curve3, Fit, Fractal, Implicit3, Mode, NewValue, Ode, ParamSurface,
    Parameter, Parametric, Region, RowAnalysis, RowInput, RowKind, Seed, Surface, Value,
    MAX_ODE_ORDER,
};
pub use compile::Program;
pub use interval::Interval;
pub use matrix::{Ink, Matrix, Stroke};
pub use parser::parse;
