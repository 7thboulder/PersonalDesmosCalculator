//! The expression engine: reads each row's LaTeX the way Desmos does,
//! resolves names across rows, and compiles curves into fast programs.

pub mod analyze;
pub mod ast;
pub mod builtins;
pub mod compile;
pub mod format;
pub mod lexer;
pub mod parser;

pub use analyze::{analyze, Axis, Curve, RowAnalysis, RowInput, RowKind};
pub use compile::Program;
pub use parser::parse;
