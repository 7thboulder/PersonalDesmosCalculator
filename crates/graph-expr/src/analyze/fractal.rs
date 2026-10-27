//! Rows that draw iteration rather than a relation: the Mandelbrot set
//! `mandelbrot()`, a Julia set `julia(c)`, a cobweb diagram
//! `cobweb(f, x_0, n)` and a bifurcation diagram `bifurcation(f, x_0)`.
//! Each takes a whole row, as `polygon` would if it could not be used in a
//! calculation. The sets and the bifurcation diagram depend on the view, so
//! the row only says what to draw and the renderer works out every pixel;
//! a cobweb is a fixed path, worked out here.

use super::*;

/// What a fractal row draws over the plane.
#[derive(Clone, Debug, PartialEq)]
pub enum Fractal {
    /// The c for which z → z² + c, from z = 0, stays bounded. With no
    /// iteration count, the renderer picks one from the zoom.
    Mandelbrot { iterations: Option<u32> },
    /// The z for which z → z² + c stays bounded, for one c (real, imaginary).
    Julia {
        c: [f64; 2],
        iterations: Option<u32>,
    },
    /// Where x → f(r, x) settles, from x = `start`, for each r along the x
    /// axis. The program reads r as `Arg::X` and x as `Arg::Y`.
    Bifurcation { program: Program, start: f64 },
}

/// The most iterations a set can ask for; past this a frame takes seconds.
const MAX_ITERATIONS: f64 = 100_000.0;
/// The most steps a cobweb takes.
const MAX_STEPS: f64 = 10_000.0;
/// A cobweb's steps when the row doesn't say.
const COBWEB_STEPS: usize = 20;
/// Where a bifurcation diagram's orbits start when the row doesn't say.
const BIFURCATION_START: f64 = 0.5;

/// Whether `f` draws a whole row rather than giving a value.
pub(super) fn draws_row(f: ShapeFn) -> bool {
    matches!(
        f,
        ShapeFn::Mandelbrot | ShapeFn::Julia | ShapeFn::Cobweb | ShapeFn::Bifurcation
    )
}

/// The error for using one inside a calculation.
pub(super) fn whole_row(f: ShapeFn) -> String {
    format!("'{}' draws on a row of its own", name(f))
}

fn name(f: ShapeFn) -> &'static str {
    match f {
        ShapeFn::Mandelbrot => "mandelbrot",
        ShapeFn::Julia => "julia",
        ShapeFn::Cobweb => "cobweb",
        _ => "bifurcation",
    }
}

impl Env {
    pub(super) fn fractal_row(&mut self, id: u64, f: ShapeFn, args: &[Expr]) -> RowAnalysis {
        let mut row = RowAnalysis::new(id, RowKind::Curve);
        let mut problems = Problems::default();
        let drawn = match f {
            ShapeFn::Mandelbrot => self.mandelbrot(args, &mut problems),
            ShapeFn::Julia => self.julia(args, &mut problems),
            ShapeFn::Cobweb => self.cobweb(args, &mut problems),
            _ => self.bifurcation(args, &mut problems),
        };
        row.missing = slider_names(&problems.missing);
        row.error = problems.first_message();
        match drawn {
            _ if row.error.is_some() => {}
            Ok(Drawn::Fractal(fractal)) => row.fractals.push(fractal),
            Ok(Drawn::Paths(paths)) => row.paths = paths,
            Err(e) => row.error = Some(e),
        }
        row
    }

    fn mandelbrot(&mut self, args: &[Expr], problems: &mut Problems) -> Result<Drawn, String> {
        let iterations = match args {
            [] => None,
            [n] => self.iterations(n, problems)?,
            _ => return Err("Try mandelbrot() or mandelbrot(n), with n iterations".into()),
        };
        Ok(Drawn::Fractal(Fractal::Mandelbrot { iterations }))
    }

    fn julia(&mut self, args: &[Expr], problems: &mut Problems) -> Result<Drawn, String> {
        let (c, n) = match args {
            [c] => (c, None),
            [c, n] => (c, Some(n)),
            _ => return Err("Try julia(c), with c a complex number such as -0.8+0.156i".into()),
        };
        let c = match self.constant(c, problems) {
            Some(Value::Number(a)) => [a, 0.0],
            Some(Value::Complex(a, b) | Value::Point(a, b)) => [a, b],
            None => return Ok(Drawn::Paths(Vec::new())),
            _ => {
                return Err("julia(c) needs c to be one complex number, such as -0.8+0.156i".into())
            }
        };
        let iterations = match n {
            Some(n) => self.iterations(n, problems)?,
            None => None,
        };
        Ok(Drawn::Fractal(Fractal::Julia { c, iterations }))
    }

    fn cobweb(&mut self, args: &[Expr], problems: &mut Problems) -> Result<Drawn, String> {
        let usage = "Try cobweb(f, x_0, n): f defined as f(x) = …, starting at x_0, for n steps";
        let (f, start, steps) = match args {
            [f, start] => (f, start, None),
            [f, start, n] => (f, start, Some(n)),
            _ => return Err(usage.into()),
        };
        let program = self.function_program(f, 1, usage, problems)?;
        let starts = match self.constant(start, problems) {
            Some(Value::Number(v)) => vec![v],
            Some(Value::Numbers(v)) => v,
            None => return Ok(Drawn::Paths(Vec::new())),
            _ => return Err("A cobweb starts at a number".into()),
        };
        let steps = match steps {
            None => COBWEB_STEPS,
            Some(n) => match self.constant(n, problems) {
                Some(Value::Number(n)) if n >= 0.0 => n.round().min(MAX_STEPS) as usize,
                None => return Ok(Drawn::Paths(Vec::new())),
                _ => return Err("A cobweb's steps must be a number, 0 or more".into()),
            },
        };
        let Some(program) = program else {
            return Ok(Drawn::Paths(Vec::new()));
        };
        let paths = starts
            .into_iter()
            .map(|x| cobweb_path(&|x| program.eval(x), x, steps))
            .collect();
        Ok(Drawn::Paths(paths))
    }

    fn bifurcation(&mut self, args: &[Expr], problems: &mut Problems) -> Result<Drawn, String> {
        let usage = "Try bifurcation(f, x_0): f defined as f(r, x) = …, with r along the x axis";
        let (f, start) = match args {
            [f] => (f, None),
            [f, start] => (f, Some(start)),
            _ => return Err(usage.into()),
        };
        let program = self.function_program(f, 2, usage, problems)?;
        let start = match start {
            None => BIFURCATION_START,
            Some(e) => match self.constant(e, problems) {
                Some(Value::Number(v)) => v,
                None => return Ok(Drawn::Paths(Vec::new())),
                _ => return Err("A bifurcation diagram starts at a number".into()),
            },
        };
        let Some(program) = program else {
            return Ok(Drawn::Paths(Vec::new()));
        };
        if crate::wgsl::body(&program).is_none() {
            return Err(
                "This function can't run on the graphics card; use arithmetic and the usual functions"
                    .into(),
            );
        }
        Ok(Drawn::Fractal(Fractal::Bifurcation { program, start }))
    }

    /// The value of an argument that must not depend on anything, or
    /// `None` when it reads an undefined name (left in `problems`).
    fn constant(&mut self, e: &Expr, problems: &mut Problems) -> Option<Value> {
        let before = problems.missing.len() + problems.errors.len();
        let val = self.lower(e, &HashMap::new(), NO_FREE, problems);
        if problems.missing.len() + problems.errors.len() > before {
            return None;
        }
        val.constant()
    }

    fn iterations(&mut self, n: &Expr, problems: &mut Problems) -> Result<Option<u32>, String> {
        match self.constant(n, problems) {
            Some(Value::Number(n)) if n >= 1.0 => Ok(Some(n.round().min(MAX_ITERATIONS) as u32)),
            None => Ok(None),
            _ => Err("The number of iterations must be 1 or more".into()),
        }
    }

    /// The function a row names, as in `cobweb(f, …)`, compiled with its
    /// parameters read as `Arg::X` and then `Arg::Y`. `None` when its body
    /// reads an undefined name (left in `problems`).
    fn function_program(
        &mut self,
        f: &Expr,
        arity: usize,
        usage: &str,
        problems: &mut Problems,
    ) -> Result<Option<Program>, String> {
        let Expr::Var(name) = f else {
            return Err(usage.into());
        };
        let Some(def) = self.funcs.get(name) else {
            return Err(format!("'{name}' isn't a function. {usage}"));
        };
        if def.params.len() != arity {
            return Err(match arity {
                1 => format!("'{name}' needs one argument, as in {name}(x) = …"),
                _ => format!("'{name}' needs two arguments, as in {name}(r, x) = …"),
            });
        }
        let free: Free = if arity == 1 {
            &[("x", Arg::X)]
        } else {
            &[("x", Arg::X), ("y", Arg::Y)]
        };
        let call = Expr::Apply(
            name.clone(),
            free.iter()
                .map(|(v, _)| Expr::Var((*v).to_owned()))
                .collect(),
        );
        let before = problems.missing.len() + problems.errors.len();
        let val = self.lower(&call, &HashMap::new(), free, problems);
        if problems.missing.len() + problems.errors.len() > before {
            return Ok(None);
        }
        match val.items.as_slice() {
            [Item::Num(low)] if !val.list => Program::compile(low).map(Some),
            _ => Err(format!("'{name}' must give one number")),
        }
    }
}

enum Drawn {
    Fractal(Fractal),
    Paths(Vec<Vec<[f64; 2]>>),
}

/// The cobweb of x → f(x) from `x`: up (or down) from the x axis to the
/// curve, across to the line y = x, and again, `steps` times. Stops early
/// where f is undefined.
pub fn cobweb_path(f: &dyn Fn(f64) -> f64, mut x: f64, steps: usize) -> Vec<[f64; 2]> {
    let mut path = vec![[x, 0.0]];
    for _ in 0..steps {
        let y = f(x);
        if !y.is_finite() {
            break;
        }
        path.push([x, y]);
        path.push([y, y]);
        x = y;
    }
    path
}
