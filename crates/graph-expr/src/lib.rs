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
pub mod units;
pub mod wgsl;

pub use analyze::{
    analyze, analyze_cached, analyze_mode, Axis, Cache, ColumnAnalysis, ColumnInput, Compare,
    ComplexPlot, Curve, Curve3, Fit, Implicit3, Mode, Ode, ParamSurface, Parameter, Parametric,
    Region, RowAnalysis, RowInput, RowKind, Seed, Surface, Value, MAX_ODE_ORDER,
};
pub use compile::Program;
pub use interval::Interval;
pub use parser::parse;
