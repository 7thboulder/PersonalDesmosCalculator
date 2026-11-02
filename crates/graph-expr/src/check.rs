//! Checks a group of steps in a document: does each line follow from the
//! one above it?
//!
//! - A line that starts with a relation (`= …`, `< …`) continues the line
//!   above: its first side is compared with the last side above. Sides of
//!   one line (`= a = b`) are compared in turn too.
//! - Expressions are compared at many points, with every free letter given
//!   values, negative ones included. Indefinite integrals and a constant C
//!   only need to agree up to a constant.
//! - An equation that follows an equation must have the same solutions:
//!   each line's solutions in its main letter are found numerically and
//!   compared, so a lost or gained solution is named.
//!
//! A step that can't be worked out (a function nobody defined, words) is
//! left unmarked rather than flagged.

use crate::ast::{Expr, Statement};
use crate::{analyze, RowInput, Value};

/// What the check says about one line.
#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    /// The first line of a group, or a line the check can't work with.
    Unknown,
    Follows,
    /// Doesn't follow, and why, in plain words.
    Wrong(String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Rel {
    Eq,
    Lt,
    Gt,
    Le,
    Ge,
}

/// One line split at its relations: `= a < b` is a leading `=`, then `a`,
/// then `<`, `b`.
struct Line {
    lead: Option<Rel>,
    sides: Vec<String>,
    rels: Vec<Rel>,
}

/// Checks each line of a group against the one before it. `context` is
/// the document's live lines, whose definitions the steps may use.
pub fn check_group(lines: &[String], context: &[String]) -> Vec<Verdict> {
    let ctx = Context::new(context);
    let mut out = Vec::with_capacity(lines.len());
    // The last side written, and the equation the line above stands for.
    let mut last_side: Option<String> = None;
    let mut equation: Option<(String, String)> = None;
    let mut anchor: Option<String> = None;
    // The state after the last line that wasn't wrong.
    let mut good: Option<(Option<String>, Option<(String, String)>)> = None;
    let mut last_wrong = false;
    for (k, latex) in lines.iter().enumerate() {
        let Some(line) = split(latex) else {
            out.push(Verdict::Unknown);
            last_side = None;
            equation = None;
            continue;
        };
        let mut verdict = Verdict::Unknown;
        let mut note = |v: Verdict| {
            // The first problem found is the one shown.
            if !matches!(verdict, Verdict::Wrong(_)) {
                if v != Verdict::Unknown || verdict == Verdict::Unknown {
                    verdict = v;
                }
            }
        };
        if k > 0 {
            let against = |last_side: &Option<String>, equation: &Option<(String, String)>| match (
                line.lead, last_side,
            ) {
                (Some(rel), Some(prev)) => ctx.compare(prev, &line.sides[0], rel, "the line above"),
                (None, _) if line.sides.len() >= 2 && line.rels.iter().all(|r| *r == Rel::Eq) => {
                    let eq = (
                        line.sides[0].clone(),
                        line.sides[line.sides.len() - 1].clone(),
                    );
                    match equation {
                        Some(prev) => ctx.same_solutions(prev, &eq),
                        None => Verdict::Unknown,
                    }
                }
                (None, Some(prev)) if line.sides.len() == 1 && equation.is_none() => {
                    ctx.compare(prev, &line.sides[0], Rel::Eq, "the line above")
                }
                _ => Verdict::Unknown,
            };
            let mut v = against(&last_side, &equation);
            // After a wrong line, a line that follows the last right one
            // is right: the slip was fixed.
            if let (Verdict::Wrong(_), true, Some((gs, ge))) = (&v, last_wrong, &good) {
                if against(gs, ge) == Verdict::Follows {
                    v = Verdict::Follows;
                }
            }
            note(v);
        }
        // Sides within the line: a = b = c checks b = c (a = b is what the
        // line says, unless it continues the line above). When the next
        // line carries on with = and the first side isn't a name such as
        // I or f(x), the line is a chain of equal things from the start.
        let chain = line.lead.is_none()
            && lines
                .get(k + 1)
                .and_then(|n| split(n))
                .is_some_and(|n| n.lead == Some(Rel::Eq))
            && line.rels.iter().all(|r| *r == Rel::Eq)
            && !is_name(&line.sides[0]);
        let from = if line.lead.is_some() || chain { 0 } else { 1 };
        for j in from..line.sides.len().saturating_sub(1) {
            note(ctx.compare(
                &line.sides[j],
                &line.sides[j + 1],
                line.rels[j],
                "the side before it",
            ));
        }
        out.push(verdict);
        // What the next line continues from.
        match line.lead {
            Some(_) => {
                if let Some(a) = &anchor {
                    let all_eq =
                        line.lead == Some(Rel::Eq) && line.rels.iter().all(|r| *r == Rel::Eq);
                    equation =
                        all_eq.then(|| (a.clone(), line.sides.last().cloned().unwrap_or_default()));
                }
            }
            None => {
                anchor = Some(line.sides[0].clone());
                equation =
                    (line.sides.len() >= 2 && line.rels.iter().all(|r| *r == Rel::Eq)).then(|| {
                        (
                            line.sides[0].clone(),
                            line.sides.last().cloned().unwrap_or_default(),
                        )
                    });
            }
        }
        last_side = line.sides.last().cloned();
        last_wrong = matches!(out.last(), Some(Verdict::Wrong(_)));
        if !last_wrong {
            good = Some((last_side.clone(), equation.clone()));
        } else if chain {
            // A wrong chain is right as far as its first side.
            good = Some((Some(line.sides[0].clone()), None));
        }
    }
    out
}

/// Splits a line at its relations outside brackets.
fn split(latex: &str) -> Option<Line> {
    let s = latex.trim();
    if s.is_empty() || s.contains("\\text") {
        return None;
    }
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut parts = Vec::new();
    let mut rels = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        let rest = &s[i..];
        let rel = if bytes[i] == b'{' {
            depth += 1;
            None
        } else if bytes[i] == b'}' {
            depth -= 1;
            None
        } else if rest.starts_with("\\left") {
            depth += 1;
            i += 5;
            continue;
        } else if rest.starts_with("\\right") {
            depth -= 1;
            i += 6;
            continue;
        } else if depth != 0 {
            None
        } else if bytes[i] == b'=' {
            Some((Rel::Eq, 1))
        } else if bytes[i] == b'<' {
            Some((Rel::Lt, 1))
        } else if bytes[i] == b'>' {
            Some((Rel::Gt, 1))
        } else {
            let cmd = |name: &str| {
                rest.strip_prefix(name)
                    .is_some_and(|r| !r.starts_with(|c: char| c.is_ascii_alphabetic()))
                    .then_some(name.len())
            };
            if let Some(n) = cmd("\\leq").or_else(|| cmd("\\le")) {
                Some((Rel::Le, n))
            } else if let Some(n) = cmd("\\geq").or_else(|| cmd("\\ge")) {
                Some((Rel::Ge, n))
            } else if let Some(n) = cmd("\\lt") {
                Some((Rel::Lt, n))
            } else if let Some(n) = cmd("\\gt") {
                Some((Rel::Gt, n))
            } else if cmd("\\ne").is_some() || cmd("\\neq").is_some() || cmd("\\approx").is_some() {
                return None;
            } else {
                None
            }
        };
        match rel {
            Some((r, n)) => {
                parts.push(s[start..i].trim().to_owned());
                rels.push(r);
                i += n;
                start = i;
            }
            None => i += 1,
        }
    }
    parts.push(s[start..].trim().to_owned());
    let lead = if parts[0].is_empty() {
        parts.remove(0);
        Some(rels.remove(0))
    } else {
        None
    };
    if parts.iter().any(String::is_empty) {
        return None;
    }
    Some(Line {
        lead,
        sides: parts,
        rels,
    })
}

// ---------------------------------------------------------------------------
// Working things out

/// Values for free letters, by letter.
const SAMPLES: [[f64; 5]; 8] = [
    [0.37, 1.13, 2.41, 0.58, 1.83],
    [1.21, 0.46, 1.77, 2.93, 0.71],
    [-0.58, 0.91, -1.37, 1.52, -0.66],
    [2.73, -1.19, 0.64, -0.42, 2.17],
    [0.83, 2.05, 0.29, 1.41, 3.07],
    [-1.71, 0.33, 2.62, -2.21, 0.97],
    [3.7, 1.61, -0.87, 0.52, 1.29],
    [0.19, -0.74, 1.08, 2.36, -1.53],
];

/// Letters that name functions when written before brackets.
const FUNCTION_LETTERS: [&str; 9] = ["f", "g", "h", "F", "G", "H", "u", "v", "w"];

struct Context {
    rows: Vec<String>,
    /// Names the live lines define.
    defined: Vec<String>,
    functions: Vec<String>,
}

impl Context {
    fn new(context: &[String]) -> Context {
        let mut defined = Vec::new();
        let mut functions = Vec::new();
        for l in context {
            match crate::parse(l) {
                Ok(Statement::Relation(Expr::Var(n), crate::ast::Relation::Eq, _)) => {
                    defined.push(n)
                }
                Ok(Statement::Relation(Expr::Apply(f, _), crate::ast::Relation::Eq, _)) => {
                    functions.push(f)
                }
                _ => {}
            }
        }
        Context {
            rows: context.to_vec(),
            defined,
            functions,
        }
    }

    /// The letters `latex` reads that nothing defines, or None when it
    /// can't be worked out (it doesn't parse, or uses an unknown function).
    fn free(&self, latex: &str) -> Option<(Vec<String>, bool)> {
        let e = match crate::parse(latex).ok()? {
            Statement::Expr(e) => e,
            _ => return None,
        };
        let mut names: Vec<String> = Vec::new();
        let mut unknown_function = false;
        let mut indefinite = false;
        e.visit_names(&mut |n, applied| {
            if n == crate::ast::INDEFINITE {
                indefinite = true;
                return;
            }
            if applied {
                if self.functions.iter().any(|f| f == n) || crate::builtins::from_name(n).is_some()
                {
                    return;
                }
                // Γ(x), ψ(x), J_n(x) and the other letter functions, when
                // no row sets the letter.
                let letter = n.split_once('_').map_or(n, |(l, _)| l);
                if !self.defined.iter().any(|d| d == letter)
                    && ((1..=2).any(|k| crate::builtins::from_letter(n, k).is_some())
                        || (n.contains('_') && crate::builtins::bessel(letter).is_some()))
                {
                    return;
                }
                // f(x) with no f defined can't be worked out; x(x+2) is a
                // product, as the calculator reads it.
                if FUNCTION_LETTERS.contains(&n) {
                    unknown_function = true;
                    return;
                }
            }
            let n = n.trim_end_matches('\'');
            if matches!(n, "pi" | "e" | "i" | "infty" | "tau")
                || self.defined.iter().any(|d| d == n)
                || names.iter().any(|m| m == n)
            {
                return;
            }
            names.push(n.to_owned());
        });
        if unknown_function {
            return None;
        }
        let constant = indefinite || names.iter().any(|n| n == "C" || n.starts_with("C_"));
        Some((names, constant))
    }

    /// The values of `a` and `b` at the same points, worked out together.
    fn values(
        &self,
        a: &str,
        b: &str,
        vars: &[String],
        points: &[Vec<f64>],
    ) -> Vec<(Option<f64>, Option<f64>)> {
        let params: Vec<String> = vars.iter().map(|v| name_latex(v)).collect();
        let head = if params.is_empty() {
            String::new()
        } else {
            format!("\\left({}\\right)", params.join(","))
        };
        let mut rows = self.rows.clone();
        rows.push(format!("A_{{chk}}{head}={a}"));
        rows.push(format!("B_{{chk}}{head}={b}"));
        let base = rows.len();
        for p in points {
            let args = if p.is_empty() {
                String::new()
            } else {
                format!(
                    "\\left({}\\right)",
                    p.iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            };
            rows.push(format!("A_{{chk}}{args}"));
            rows.push(format!("B_{{chk}}{args}"));
        }
        let vals = numbers(&rows);
        (0..points.len())
            .map(|k| (vals[base + 2 * k], vals[base + 2 * k + 1]))
            .collect()
    }

    /// Whether `b` is related to `a` by `rel` (b = a, a < b, …).
    fn compare(&self, a: &str, b: &str, rel: Rel, what: &str) -> Verdict {
        let (Some((fa, ca)), Some((fb, cb))) = (self.free(a), self.free(b)) else {
            return Verdict::Unknown;
        };
        let mut vars = fa;
        for n in fb {
            if !vars.contains(&n) {
                vars.push(n);
            }
        }
        if vars.len() > SAMPLES[0].len() {
            return Verdict::Unknown;
        }
        let up_to_constant = rel == Rel::Eq && (ca || cb);
        // C is the constant itself: the same value on both sides.
        let points: Vec<Vec<f64>> = SAMPLES
            .iter()
            .map(|row| {
                vars.iter()
                    .enumerate()
                    .map(|(i, v)| {
                        if up_to_constant && (v == "C" || v.starts_with("C_")) {
                            0.0
                        } else {
                            row[i]
                        }
                    })
                    .collect()
            })
            .collect();
        let vals = self.values(a, b, &vars, &points);
        let mut used = 0;
        let mut offset: Option<f64> = None;
        for (k, (va, vb)) in vals.iter().enumerate() {
            let (Some(x), Some(y)) = (va, vb) else {
                continue;
            };
            used += 1;
            let tol = 1e-7 * (1.0 + x.abs().max(y.abs()));
            let at = || where_(&vars, &points[k]);
            let ok = match rel {
                Rel::Eq if up_to_constant => {
                    let d = y - x;
                    match offset {
                        None => {
                            offset = Some(d);
                            true
                        }
                        Some(o) => (d - o).abs() <= 1e-6 * (1.0 + x.abs().max(y.abs())),
                    }
                }
                Rel::Eq => (x - y).abs() <= tol,
                // a < b: the new side is bigger.
                Rel::Lt => y - x > -tol,
                Rel::Le => y - x >= -tol,
                Rel::Gt => x - y > -tol,
                Rel::Ge => x - y >= -tol,
            };
            if !ok {
                return Verdict::Wrong(match rel {
                    Rel::Eq if up_to_constant => {
                        format!("Not the same as {what}, even up to a constant{}", at())
                    }
                    Rel::Eq => format!(
                        "Not equal to {what}{}: this is {}, not {}",
                        at(),
                        show(*y),
                        show(*x)
                    ),
                    _ => format!(
                        "Doesn't hold{}: {what} is {}, this is {}",
                        at(),
                        show(*x),
                        show(*y)
                    ),
                });
            }
        }
        if used >= 2 || (used >= 1 && vars.is_empty()) {
            Verdict::Follows
        } else {
            Verdict::Unknown
        }
    }

    /// Whether two equations have the same solutions in their main letter,
    /// for a few values of any other letters.
    fn same_solutions(&self, before: &(String, String), after: &(String, String)) -> Verdict {
        let g = |e: &(String, String)| format!("\\left({}\\right)-\\left({}\\right)", e.0, e.1);
        let (gb, ga) = (g(before), g(after));
        let (Some((fb, _)), Some((fa, _))) = (self.free(&gb), self.free(&ga)) else {
            return Verdict::Unknown;
        };
        let mut vars = fb;
        for n in fa {
            if !vars.contains(&n) {
                vars.push(n);
            }
        }
        if vars.is_empty() {
            // Both sides are numbers: a true statement stays true.
            return self
                .compare(&before.0, &before.1, Rel::Eq, "its other side")
                .and_then_true(|| self.compare(&after.0, &after.1, Rel::Eq, "its other side"));
        }
        let main = ["x", "y", "t", "theta", "r", "n"]
            .iter()
            .find(|m| vars.iter().any(|v| v == *m))
            .map(|m| m.to_string())
            .unwrap_or_else(|| vars[0].clone());
        let others: Vec<String> = vars.iter().filter(|v| **v != main).cloned().collect();
        if others.len() > 3 {
            return Verdict::Unknown;
        }
        let mut checked = false;
        for row in SAMPLES.iter().take(2) {
            let fixed: Vec<f64> = (0..others.len()).map(|i| row[i]).collect();
            let rb = self.roots(&gb, &main, &others, &fixed);
            let ra = self.roots(&ga, &main, &others, &fixed);
            let (Some(rb), Some(ra)) = (rb, ra) else {
                continue;
            };
            checked = true;
            let near =
                |r: f64, set: &[f64]| set.iter().any(|s| (s - r).abs() <= 1e-5 * (1.0 + r.abs()));
            let given = if others.is_empty() {
                String::new()
            } else {
                format!(" when {}", where_list(&others, &fixed))
            };
            if let Some(r) = rb.iter().find(|r| !near(**r, &ra)) {
                return Verdict::Wrong(format!(
                    "Lost the solution {} = {}{given}",
                    show_name(&main),
                    show(*r)
                ));
            }
            if let Some(r) = ra.iter().find(|r| !near(**r, &rb)) {
                return Verdict::Wrong(format!(
                    "Gained the solution {} = {}{given}, which the line above doesn't have",
                    show_name(&main),
                    show(*r)
                ));
            }
        }
        if checked {
            Verdict::Follows
        } else {
            Verdict::Unknown
        }
    }

    /// The real solutions of g = 0 in `main` between -20 and 20, with the
    /// other letters fixed. None when g can't be worked out there.
    fn roots(&self, g: &str, main: &str, others: &[String], fixed: &[f64]) -> Option<Vec<f64>> {
        const N: usize = 1600;
        const LO: f64 = -20.0;
        const HI: f64 = 20.0;
        // Offset so that round numbers fall between samples rarely.
        let xs: Vec<f64> = (0..=N)
            .map(|i| LO + (HI - LO) * i as f64 / N as f64 + 1.234_567e-4)
            .collect();
        let ys = self.at_list(g, main, others, fixed, &xs)?;
        let defined = ys.iter().filter(|y| y.is_some()).count();
        if defined < N / 4 {
            return None;
        }
        let scale = ys
            .iter()
            .flatten()
            .map(|y| y.abs())
            .fold(0.0f64, f64::max)
            .max(1.0);
        // Brackets: a sign change, or a tiny value at a local minimum of |g|.
        let mut brackets = Vec::new();
        for i in 0..N {
            let (Some(a), Some(b)) = (ys[i], ys[i + 1]) else {
                continue;
            };
            if a == 0.0 {
                brackets.push((xs[i], xs[i]));
            } else if a.signum() != b.signum() && b != 0.0 {
                brackets.push((xs[i], xs[i + 1]));
            } else if i > 0 {
                if let Some(p) = ys[i - 1] {
                    if a.abs() < p.abs() && a.abs() <= b.abs() && a.abs() < 1e-6 * scale {
                        brackets.push((xs[i - 1], xs[i + 1]));
                    }
                }
            }
        }
        if brackets.len() > 60 {
            return None;
        }
        // Bisection on all brackets at once; a double root's bracket is
        // narrowed by the smaller |g| instead.
        let mut lo: Vec<f64> = brackets.iter().map(|b| b.0).collect();
        let mut hi: Vec<f64> = brackets.iter().map(|b| b.1).collect();
        let sign_change: Vec<bool> = brackets
            .iter()
            .map(|b| b.0 != b.1 && (b.1 - b.0) < 2.0 * (HI - LO) / N as f64)
            .collect();
        for _ in 0..45 {
            if lo.is_empty() {
                break;
            }
            let mids: Vec<f64> = lo.iter().zip(&hi).map(|(a, b)| (a + b) / 2.0).collect();
            let lows = self.at_list(g, main, others, fixed, &lo)?;
            let vm = self.at_list(g, main, others, fixed, &mids)?;
            for k in 0..lo.len() {
                let (Some(a), Some(m)) = (lows[k], vm[k]) else {
                    continue;
                };
                if sign_change[k] {
                    if a.signum() == m.signum() {
                        lo[k] = mids[k];
                    } else {
                        hi[k] = mids[k];
                    }
                } else {
                    // Keep the half nearer the smaller value.
                    let q1 = (lo[k] + mids[k]) / 2.0;
                    let q3 = (mids[k] + hi[k]) / 2.0;
                    let _ = (q1, q3, a);
                    let w = (hi[k] - lo[k]) / 4.0;
                    lo[k] = mids[k] - w;
                    hi[k] = mids[k] + w;
                }
            }
        }
        let mids: Vec<f64> = lo.iter().zip(&hi).map(|(a, b)| (a + b) / 2.0).collect();
        let vals = self.at_list(g, main, others, fixed, &mids)?;
        let mut out: Vec<f64> = Vec::new();
        for (m, v) in mids.iter().zip(vals) {
            // A jump through infinity, as 1/x has, isn't a solution.
            if v.is_some_and(|v| v.abs() <= 1e-6 * scale) {
                let r = tidy(*m);
                if !out.iter().any(|o| (o - r).abs() <= 1e-6 * (1.0 + r.abs())) {
                    out.push(r);
                }
            }
        }
        Some(out)
    }

    /// g at each of `xs` for `main`, the others fixed.
    fn at_list(
        &self,
        g: &str,
        main: &str,
        others: &[String],
        fixed: &[f64],
        xs: &[f64],
    ) -> Option<Vec<Option<f64>>> {
        if xs.is_empty() {
            return Some(Vec::new());
        }
        let mut params = vec![name_latex(main)];
        params.extend(others.iter().map(|o| name_latex(o)));
        let mut rows = self.rows.clone();
        rows.push(format!("G_{{chk}}\\left({}\\right)={g}", params.join(",")));
        let list = xs
            .iter()
            .map(|x| format!("{x}"))
            .collect::<Vec<_>>()
            .join(",");
        let mut args = vec![format!("\\left[{list}\\right]")];
        args.extend(fixed.iter().map(|v| v.to_string()));
        rows.push(format!("G_{{chk}}\\left({}\\right)", args.join(",")));
        let input: Vec<RowInput> = rows
            .iter()
            .enumerate()
            .map(|(i, l)| RowInput {
                id: i as u64,
                latex: l.clone(),
                ..Default::default()
            })
            .collect();
        let out = std::panic::catch_unwind(|| analyze(&input)).ok()?;
        match &out.last()?.value {
            Some(Value::Numbers(v)) if v.len() == xs.len() => {
                Some(v.iter().map(|y| y.is_finite().then_some(*y)).collect())
            }
            Some(Value::Number(y)) if xs.len() == 1 => Some(vec![y.is_finite().then_some(*y)]),
            _ => None,
        }
    }
}

impl Verdict {
    fn and_then_true(self, next: impl FnOnce() -> Verdict) -> Verdict {
        match self {
            Verdict::Follows => match next() {
                Verdict::Wrong(_) => {
                    Verdict::Wrong("This is false, but the line above is true".into())
                }
                v => v,
            },
            Verdict::Wrong(_) => match next() {
                Verdict::Follows => {
                    Verdict::Wrong("This is true, but the line above is false".into())
                }
                _ => Verdict::Unknown,
            },
            Verdict::Unknown => Verdict::Unknown,
        }
    }
}

/// Whether a side is just a name, or a function's name and letters: what
/// a line defines rather than claims.
fn is_name(side: &str) -> bool {
    match crate::parse(side) {
        Ok(Statement::Expr(Expr::Var(_))) => true,
        Ok(Statement::Expr(Expr::Apply(_, args))) => args.iter().all(|a| matches!(a, Expr::Var(_))),
        _ => false,
    }
}

/// Real values of rows; the ones that aren't real numbers are None.
fn numbers(rows: &[String]) -> Vec<Option<f64>> {
    let input: Vec<RowInput> = rows
        .iter()
        .enumerate()
        .map(|(i, l)| RowInput {
            id: i as u64,
            latex: l.clone(),
            ..Default::default()
        })
        .collect();
    let out = std::panic::catch_unwind(|| analyze(&input)).unwrap_or_default();
    let mut vals: Vec<Option<f64>> = out
        .iter()
        .map(|r| match &r.value {
            Some(Value::Number(v)) if v.is_finite() => Some(*v),
            _ => None,
        })
        .collect();
    vals.resize(rows.len(), None);
    vals
}

/// The LaTeX for a name as the parser gives it: a_1 as a_{1}, theta as
/// \theta.
fn name_latex(name: &str) -> String {
    crate::symbolic::latex::name(name)
}

fn show_name(name: &str) -> String {
    match name {
        "theta" => "θ".into(),
        "pi" => "π".into(),
        n => n.to_owned(),
    }
}

/// A number for a message: whole numbers as they are, else 6 digits.
fn show(v: f64) -> String {
    let r = tidy(v);
    if r == r.round() && r.abs() < 1e12 {
        return format!("{}", r as i64);
    }
    let s = format!("{:.6}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    s.to_owned()
}

/// Rounds a solution found numerically to a nearby whole number or
/// simple fraction.
fn tidy(v: f64) -> f64 {
    for d in [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0] {
        let r = (v * d).round() / d;
        if (r - v).abs() < 1e-7 {
            return r;
        }
    }
    v
}

fn where_(vars: &[String], at: &[f64]) -> String {
    if vars.is_empty() {
        String::new()
    } else {
        format!(" at {}", where_list(vars, at))
    }
}

fn where_list(vars: &[String], at: &[f64]) -> String {
    vars.iter()
        .zip(at)
        .map(|(v, x)| format!("{} = {}", show_name(v), show(*x)))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(lines: &[&str]) -> Vec<Verdict> {
        let lines: Vec<String> = lines.iter().map(|s| s.to_string()).collect();
        check_group(&lines, &[])
    }

    fn wrong(v: &Verdict) -> &str {
        match v {
            Verdict::Wrong(r) => r,
            other => panic!("expected a problem, got {other:?}"),
        }
    }

    #[test]
    fn equal_steps_follow() {
        let v = check(&[
            "\\left(x+1\\right)^2",
            "=x^2+2x+1",
            "=x\\left(x+2\\right)+1",
        ]);
        assert_eq!(
            v,
            vec![Verdict::Unknown, Verdict::Follows, Verdict::Follows]
        );
        // Without the leading =, an expression under an expression.
        let v = check(&["\\sin^2x+\\cos^2x", "1"]);
        assert_eq!(v[1], Verdict::Follows);
    }

    #[test]
    fn a_wrong_step_is_flagged() {
        let v = check(&["\\left(a+b\\right)^2", "=a^2+b^2", "=a^2+2ab+b^2"]);
        assert!(
            wrong(&v[1]).starts_with("Not equal to the line above at a = "),
            "{v:?}"
        );
        // The third line fixes the slip: it follows the first.
        assert_eq!(v[2], Verdict::Follows);
        let v = check(&["\\left(a+b\\right)^2", "=a^2+b^2", "=a^2+b^2+1"]);
        assert!(matches!(v[2], Verdict::Wrong(_)));
    }

    #[test]
    fn the_plans_integral() {
        let v = check(&[
            "I=\\int_0^{\\infty}x^2e^{-x^2}dx",
            "=\\frac{1}{2}\\int_0^{\\infty}e^{-x^2}dx",
            "=\\frac{1}{2}\\sqrt{\\pi}",
            "=\\frac{\\sqrt{\\pi}}{4}",
        ]);
        assert_eq!(v[1], Verdict::Follows, "{v:?}");
        assert!(
            wrong(&v[2]).starts_with("Not equal to the line above: this is 0.886227, not 0.443113"),
            "{v:?}"
        );
        // √π/4 is right again.
        assert_eq!(v[3], Verdict::Follows);
    }

    #[test]
    fn antiderivatives_agree_up_to_a_constant() {
        let v = check(&[
            "\\int_{ }^{ }2\\sin x\\cos x\\ dx",
            "=\\sin^2x+C",
            "=-\\frac{1}{2}\\cos\\left(2x\\right)+C",
        ]);
        assert_eq!(v[1], Verdict::Follows, "{v:?}");
        assert_eq!(v[2], Verdict::Follows, "{v:?}");
        let v = check(&["\\int_{ }^{ }x\\ dx", "=x^2+C"]);
        assert!(wrong(&v[1]).contains("even up to a constant"), "{v:?}");
    }

    #[test]
    fn equations_keep_their_solutions() {
        let v = check(&[
            "x^2+2x=3",
            "x^2+2x-3=0",
            "\\left(x+3\\right)\\left(x-1\\right)=0",
        ]);
        assert_eq!(
            v,
            vec![Verdict::Unknown, Verdict::Follows, Verdict::Follows]
        );
        // Dividing by x loses x = 0.
        let v = check(&["x^2=3x", "x=3"]);
        assert_eq!(wrong(&v[1]), "Lost the solution x = 0");
        // Squaring gains one.
        let v = check(&["x=2", "x^2=4"]);
        assert_eq!(
            wrong(&v[1]),
            "Gained the solution x = -2, which the line above doesn't have"
        );
        // With another letter.
        let v = check(&["ax+b=0", "x=-\\frac{b}{a}"]);
        assert_eq!(v[1], Verdict::Follows, "{v:?}");
        // A sign slip.
        let v = check(&["2x+6=10", "2x=16"]);
        assert!(wrong(&v[1]).starts_with("Lost the solution x = 2"), "{v:?}");
    }

    #[test]
    fn inequalities_in_a_chain() {
        let v = check(&["x^2+1", ">x^2", "\\ge0"]);
        assert_eq!(v[1], Verdict::Follows, "{v:?}");
        assert_eq!(v[2], Verdict::Follows, "{v:?}");
        let v = check(&["x^2", "\\ge x"]);
        assert!(matches!(v[1], Verdict::Wrong(_)), "{v:?}");
    }

    #[test]
    fn unknown_things_are_left_alone() {
        let v = check(&["f\\left(x\\right)", "=g\\left(x\\right)"]);
        assert_eq!(v[1], Verdict::Unknown);
        let v = check(&["x=1", "\\text{so } x=1"]);
        assert_eq!(v[1], Verdict::Unknown);
        // Definitions from live lines are used.
        let lines = vec!["f\\left(2\\right)".to_owned(), "=5".to_owned()];
        let v = check_group(&lines, &["f\\left(x\\right)=x^2+1".to_owned()]);
        assert_eq!(v[1], Verdict::Follows);
    }

    #[test]
    fn a_false_first_line_is_flagged() {
        let v = check(&["\\left(a+b\\right)^2=a^2+b^2", "=a^2+2ab+b^2"]);
        assert!(
            wrong(&v[0]).starts_with("Not equal to the side before it"),
            "{v:?}"
        );
        // The second line follows the first side.
        assert_eq!(v[1], Verdict::Follows, "{v:?}");
        // An equation followed by its own working isn't a chain.
        let v = check(&["I=\\int_0^1x\\ dx", "=\\frac{1}{2}"]);
        assert_eq!(v, vec![Verdict::Unknown, Verdict::Follows]);
    }

    #[test]
    fn sides_in_one_line() {
        let v = check(&["y=\\left(x-1\\right)^2=x^2-2x+1"]);
        assert_eq!(v[0], Verdict::Follows);
        let v = check(&["\\left(x-1\\right)^2", "=x^2-2x+1=x^2-2x"]);
        assert!(wrong(&v[1]).contains("the side before it"), "{v:?}");
    }
}
