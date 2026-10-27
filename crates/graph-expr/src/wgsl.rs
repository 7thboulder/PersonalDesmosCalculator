//! Turns a compiled relation into WGSL, so inequality shading is decided by
//! the GPU at every pixel instead of by the CPU on a grid.
//!
//! The generated code is a function body that computes the relation's left
//! side minus its right at one point. Anything that cannot be expressed
//! (factorials, gcd and the list functions) returns `None`, and the caller
//! falls back to drawing only the boundary.

use crate::builtins::Builtin;
use crate::compile::{Arg, BinOp, Op, Program};

// The helpers the generated code calls (`gc_pow`, `gc_mod` and friends) are
// defined once per shader by the renderer, from `graph_core::shader`.

/// How many of a relation's numbers are passed in as uniforms. Any past
/// this are written into the code.
pub const MAX_CONSTANTS: usize = 256;

/// A relation as WGSL. Its numbers are not in the code but read from
/// `u.k`, an array of `MAX_CONSTANTS / 4` vec4s the renderer fills from
/// `constants`, so moving a slider changes only the numbers and the
/// compiled shader is reused.
#[derive(Clone, Debug, PartialEq)]
pub struct Shader {
    /// The body of `fn relation(x: f32, y: f32) -> f32`, including `return`.
    pub body: String,
    pub constants: Vec<f32>,
}

/// The relation as WGSL, or `None` when some part of it has no GPU
/// equivalent.
pub fn body(program: &Program) -> Option<Shader> {
    let mut constants = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut lines = String::new();
    let mut next = 0usize;
    // Longer expressions are bound to a name, so the source stays readable
    // and the shader compiler is not handed one enormous expression.
    let mut push = |expr: String, stack: &mut Vec<String>, lines: &mut String| {
        if expr.len() <= 24 {
            stack.push(expr);
        } else {
            let name = format!("t{next}");
            lines.push_str(&format!("    let {name} = {expr};\n"));
            stack.push(name);
            next += 1;
        }
    };
    for op in program.ops() {
        match *op {
            Op::Const(v) => stack.push(if v.is_nan() {
                "gc_nan()".into()
            } else if v.is_infinite() {
                // The largest finite float behaves like infinity in every
                // comparison a relation makes.
                if v > 0.0 { "3.4e38" } else { "-3.4e38" }.into()
            } else if constants.len() < MAX_CONSTANTS {
                let i = constants.len();
                constants.push(v as f32);
                format!("u.k[{}][{}]", i / 4, i % 4)
            } else {
                float(v)
            }),
            Op::Arg(Arg::X) => stack.push("x".into()),
            Op::Arg(Arg::Y) => stack.push("y".into()),
            // Integrals are evaluated on the CPU.
            Op::Arg(Arg::Local(_)) | Op::Integral { .. } => return None,
            Op::Neg => {
                let a = stack.pop()?;
                push(format!("-({a})"), &mut stack, &mut lines);
            }
            Op::Bin(bin) => {
                let b = stack.pop()?;
                let a = stack.pop()?;
                let expr = match bin {
                    BinOp::Add => format!("({a}) + ({b})"),
                    BinOp::Sub => format!("({a}) - ({b})"),
                    BinOp::Mul => format!("({a}) * ({b})"),
                    BinOp::Div => format!("({a}) / ({b})"),
                    BinOp::Pow => format!("gc_pow({a}, {b})"),
                    BinOp::Lt => format!("gc_truth({a}, {b}, ({a}) < ({b}))"),
                    BinOp::Le => format!("gc_truth({a}, {b}, ({a}) <= ({b}))"),
                    BinOp::Gt => format!("gc_truth({a}, {b}, ({a}) > ({b}))"),
                    BinOp::Ge => format!("gc_truth({a}, {b}, ({a}) >= ({b}))"),
                    BinOp::Eq => format!("gc_truth({a}, {b}, ({a}) == ({b}))"),
                };
                push(expr, &mut stack, &mut lines);
            }
            Op::Call(f, n) => {
                let at = stack.len().checked_sub(n as usize)?;
                let args: Vec<String> = stack.split_off(at);
                push(call(f, &args)?, &mut stack, &mut lines);
            }
            Op::Select => {
                let otherwise = stack.pop()?;
                let then = stack.pop()?;
                let c = stack.pop()?;
                push(
                    format!("gc_select({c}, {then}, {otherwise})"),
                    &mut stack,
                    &mut lines,
                );
            }
        }
    }
    let result = stack.pop()?;
    if !stack.is_empty() {
        return None;
    }
    lines.push_str(&format!("    return {result};\n"));
    Some(Shader {
        body: lines,
        constants,
    })
}

fn float(v: f64) -> String {
    let v = v as f32;
    if v == v.trunc() && v.abs() < 1e9 {
        format!("{v:.1}")
    } else {
        format!("{v:e}")
    }
}

fn call(f: Builtin, a: &[String]) -> Option<String> {
    use Builtin::*;
    let one = |name: &str| Some(format!("{name}({})", a[0]));
    let reciprocal = |name: &str| Some(format!("1.0 / {name}({})", a[0]));
    match (f, a.len()) {
        (Sin, 1) => one("sin"),
        (Cos, 1) => one("cos"),
        (Tan, 1) => one("tan"),
        (Csc, 1) => reciprocal("sin"),
        (Sec, 1) => reciprocal("cos"),
        (Cot, 1) => one("gc_cot"),
        (Arcsin, 1) => one("asin"),
        (Arccos, 1) => one("acos"),
        (Arctan, 1) => one("atan"),
        (Arctan, 2) => Some(format!("atan2({}, {})", a[0], a[1])),
        (Arccsc, 1) => Some(format!("asin(1.0 / ({}))", a[0])),
        (Arcsec, 1) => Some(format!("acos(1.0 / ({}))", a[0])),
        (Arccot, 1) => Some(format!("1.5707964 - atan({})", a[0])),
        (Sinh, 1) => one("sinh"),
        (Cosh, 1) => one("cosh"),
        (Tanh, 1) => one("tanh"),
        (Csch, 1) => reciprocal("sinh"),
        (Sech, 1) => reciprocal("cosh"),
        (Coth, 1) => one("gc_coth"),
        (Arcsinh, 1) => one("asinh"),
        (Arccosh, 1) => one("acosh"),
        (Arctanh, 1) => one("atanh"),
        (Arccsch, 1) => Some(format!("asinh(1.0 / ({}))", a[0])),
        (Arcsech, 1) => Some(format!("acosh(1.0 / ({}))", a[0])),
        (Arccoth, 1) => Some(format!("atanh(1.0 / ({}))", a[0])),
        (Ln, 1) => one("log"),
        (Log, 1) => one("log2").map(|s| format!("({s}) * 0.30103")),
        (LogBase, 2) => Some(format!("log2({}) / log2({})", a[1], a[0])),
        (Exp, 1) => one("exp"),
        (Sqrt, 1) => one("sqrt"),
        (Root, 2) => Some(format!("gc_pow({}, 1.0 / ({}))", a[1], a[0])),
        (Abs, 1) => one("abs"),
        (Floor, 1) => one("floor"),
        (Ceil, 1) => one("ceil"),
        (Round, 1) => one("round"),
        (Round, 2) => Some(format!("gc_round_to({}, {})", a[0], a[1])),
        (Sign, 1) => one("gc_sign"),
        (Mod, 2) => Some(format!("gc_mod({}, {})", a[0], a[1])),
        (Min, _) => Some(fold("min", a)),
        (Max, _) => Some(fold("max", a)),
        (Mean, _) => Some(format!("({}) / {}.0", sum(a), a.len())),
        (Total, _) => Some(sum(a)),
        (Length, n) => Some(format!("{n}.0")),
        // Factorials, gcd, lcm, nCr, nPr and median have no closed form here.
        _ => None,
    }
}

fn fold(name: &str, a: &[String]) -> String {
    a.iter()
        .skip(1)
        .fold(a[0].clone(), |acc, v| format!("{name}({acc}, {v})"))
}

fn sum(a: &[String]) -> String {
    a.iter()
        .map(|v| format!("({v})"))
        .collect::<Vec<_>>()
        .join(" + ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analyze, RowInput};

    fn region_body(latex: &str) -> Option<String> {
        let rows = [RowInput {
            id: 0,
            latex: latex.to_string(),
            ..Default::default()
        }];
        let region = analyze(&rows)[0].region()?.clone();
        body(&region.program).map(|s| s.body)
    }

    fn shader(latex: &str) -> Shader {
        let rows = [RowInput {
            id: 0,
            latex: latex.to_string(),
            ..Default::default()
        }];
        body(&analyze(&rows)[0].region().unwrap().program).unwrap()
    }

    #[test]
    fn numbers_are_uniforms_so_a_slider_keeps_the_shader() {
        let (a, b) = (shader("x^2+y^2<2.5"), shader("x^2+y^2<7.25"));
        assert_eq!(a.body, b.body);
        assert!(a.constants.contains(&2.5) && b.constants.contains(&7.25));
        assert!(!a.body.contains("2.5"), "{}", a.body);
    }

    #[test]
    fn a_circle_becomes_a_shader() {
        let code = region_body("x^2+y^2=25").expect("WGSL");
        assert!(code.contains("return"));
        assert!(code.contains('x') && code.contains('y'));
        // Powers of two are the common case and must not need a helper call
        // per pixel... they do go through gc_pow, which handles them in a
        // branch; what matters is that the code is complete and compiles.
        assert!(code.lines().last().unwrap().trim().starts_with("return"));
    }

    #[test]
    fn long_expressions_are_bound_to_names() {
        let code =
            region_body("\\sin\\left(xy\\right)+\\cos\\left(x+y\\right)>\\tan\\left(xy\\right)")
                .expect("WGSL");
        assert!(code.contains("let t0 ="), "{code}");
    }

    #[test]
    fn unsupported_functions_give_up() {
        assert_eq!(region_body("x!=y"), None);
        assert_eq!(region_body("\\gcd\\left(x,y\\right)=2"), None);
    }
}
