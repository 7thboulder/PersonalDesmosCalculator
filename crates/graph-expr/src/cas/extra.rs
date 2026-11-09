//! More kinds of problem for Giac: systems of equations, inequalities,
//! recurrences, matrices and Fourier transforms, and assumptions written
//! as a restriction, like `\sqrt{x^2} \{x > 0\}`.

use super::*;

/// What the planner knows about the other rows.
pub(super) struct Context<'a> {
    pub defs: &'a [Statement],
    pub funcs: &'a HashSet<String>,
    pub vars: &'a HashSet<String>,
    pub conditions: &'a [Condition],
}

// ---------------------------------------------------------------------------
// Assumptions

/// `e {x > 0}` reads as `e` times a one-branch piecewise of 1: that
/// restriction, taken off, as assumptions.
fn strip(e: Expr, out: &mut Vec<Condition>) -> Expr {
    match e {
        Expr::Mul(a, b) => match *b {
            Expr::Piecewise(branches, None)
                if branches.len() == 1 && branches[0].1 == Expr::Num(1.0) =>
            {
                out.extend(branches.into_iter().map(|(c, _)| c));
                strip(*a, out)
            }
            b => Expr::Mul(a, Box::new(b)),
        },
        e => e,
    }
}

/// The statement without a trailing restriction, and the restriction's
/// conditions.
pub(super) fn take_restriction(statement: Statement) -> (Statement, Vec<Condition>) {
    let mut found = Vec::new();
    let statement = match statement {
        Statement::Expr(e) => Statement::Expr(strip(e, &mut found)),
        Statement::Relation(a, rel, b) => {
            let b = strip(b, &mut found);
            Statement::Relation(a, rel, b)
        }
        s => s,
    };
    (statement, found)
}

fn giac_relation(r: Relation) -> &'static str {
    match r {
        Relation::Eq => "=",
        Relation::Lt => "<",
        Relation::Gt => ">",
        Relation::Le => "<=",
        Relation::Ge => ">=",
    }
}

/// Each condition as Giac writes it: `a < b` or, for a chain, `a < b and
/// b < c`.
fn condition(c: &Condition, w: &mut Writer) -> Result<String, String> {
    let ops: Vec<String> = c
        .operands
        .iter()
        .map(|o| w.expr(o))
        .collect::<Result<_, _>>()?;
    Ok(c.relations
        .iter()
        .enumerate()
        .map(|(k, r)| format!("({}{}{})", ops[k], giac_relation(*r), ops[k + 1]))
        .collect::<Vec<_>>()
        .join(" and "))
}

/// Giac's `assume` lines for a restriction's conditions.
pub(super) fn assumptions(
    conditions: &[Condition],
    names: &mut Names,
    funcs: &HashSet<String>,
    vars: &HashSet<String>,
) -> Result<Vec<String>, String> {
    let mut w = Writer { names, funcs, vars };
    let mut lines = Vec::new();
    for c in conditions {
        let ops: Vec<String> = c
            .operands
            .iter()
            .map(|o| w.expr(o))
            .collect::<Result<_, _>>()?;
        for (k, r) in c.relations.iter().enumerate() {
            let verb = if lines.is_empty() {
                "assume"
            } else {
                "additionally"
            };
            lines.push(format!(
                "{verb}({}{}{})",
                ops[k],
                giac_relation(*r),
                ops[k + 1]
            ));
        }
    }
    Ok(lines)
}

// ---------------------------------------------------------------------------
// Planning

fn sorted_vars(mut names: Vec<String>) -> Vec<String> {
    names.sort_by_key(|n| match n.as_str() {
        "x" => 0,
        "y" => 1,
        "z" => 2,
        _ => 3,
    });
    names
}

/// The pieces of `latex` between commas outside any brackets.
pub(super) fn split_commas(latex: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let bytes = latex.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let rest = &latex[i..];
        if rest.starts_with("\\left") {
            depth += 1;
            i += 5;
            continue;
        }
        if rest.starts_with("\\right") {
            depth -= 1;
            i += 6;
            continue;
        }
        match bytes[i] {
            b'{' | b'(' | b'[' => depth += 1,
            b'}' | b')' | b']' => depth -= 1,
            b',' if depth == 0 => {
                parts.push(latex[start..i].trim().to_owned());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(latex[start..].trim().to_owned());
    // `\left(` counted twice (\left and the bracket), and `\right)` too,
    // so the depth stays balanced either way.
    parts
}

/// Whether another row is an equation to solve with this one.
pub(super) fn is_equation_row(s: &Statement, vars: &HashSet<String>) -> bool {
    is_equation(s, vars)
}

/// Whether a statement is an equation to solve, not a definition.
fn is_equation(s: &Statement, ctx_vars: &HashSet<String>) -> bool {
    match s {
        Statement::Relation(Expr::Apply(..), Relation::Eq, _) => false,
        Statement::Relation(Expr::Var(n), Relation::Eq, rhs) => {
            matches!(n.as_str(), "x" | "y" | "z") || rhs.mentions(n) || ctx_vars.contains(n)
        }
        Statement::Relation(_, Relation::Eq, _) => true,
        _ => false,
    }
}

/// A system: equations written in one row, split by commas, or this
/// row's equation with the other rows' equations.
pub(super) fn plan_system(
    latex: &str,
    others: &[String],
    chosen: Option<&str>,
    ctx: &Context,
) -> Option<Result<Plan, String>> {
    let parts = split_commas(latex);
    let mut eqs: Vec<(String, Expr, Expr)> = Vec::new();
    if parts.len() >= 2 {
        for p in &parts {
            match crate::parse(p) {
                Ok(Statement::Relation(a, Relation::Eq, b)) => eqs.push((p.clone(), a, b)),
                _ => return None,
            }
        }
    } else if chosen == Some("system") {
        let Ok(Statement::Relation(a, Relation::Eq, b)) = crate::parse(latex) else {
            return None;
        };
        eqs.push((latex.to_owned(), a, b));
        let mine = free_names(&Expr::Sub(
            Box::new(eqs[0].1.clone()),
            Box::new(eqs[0].2.clone()),
        ));
        for o in others {
            let Ok(s) = crate::parse(o) else { continue };
            if !is_equation(&s, &HashSet::new()) {
                continue;
            }
            let Statement::Relation(a, _, b) = s else {
                continue;
            };
            let names = free_names(&Expr::Sub(Box::new(a.clone()), Box::new(b.clone())));
            if names
                .iter()
                .any(|n| mine.contains(n) && !ctx.vars.contains(n))
            {
                eqs.push((o.clone(), a, b));
            }
        }
        if eqs.len() < 2 {
            return Some(Err(
                "There are no other equations with the same unknowns to solve with".into(),
            ));
        }
    } else {
        return None;
    }
    Some(system(eqs, latex, others, ctx))
}

fn system(
    eqs: Vec<(String, Expr, Expr)>,
    latex: &str,
    others: &[String],
    ctx: &Context,
) -> Result<Plan, String> {
    let mut names = Names::default();
    let mut used: Vec<String> = Vec::new();
    for (_, a, b) in &eqs {
        for n in free_names(&Expr::Sub(Box::new(a.clone()), Box::new(b.clone()))) {
            if !used.contains(&n) {
                used.push(n);
            }
        }
    }
    // `y = x + 1` in another row is an equation here, not a value of y.
    let set: HashSet<String> = ctx
        .vars
        .iter()
        .filter(|v| !matches!(v.as_str(), "x" | "y" | "z"))
        .cloned()
        .collect();
    let unknowns: Vec<String> =
        sorted_vars(used.iter().filter(|n| !set.contains(*n)).cloned().collect());
    if unknowns.is_empty() {
        return Err("These equations have no unknowns to solve for".into());
    }
    let mut w = Writer {
        names: &mut names,
        funcs: ctx.funcs,
        vars: ctx.vars,
    };
    let mut written = Vec::new();
    let mut residuals = Vec::new();
    for (_, a, b) in &eqs {
        let (ga, gb) = (w.expr(a)?, w.expr(b)?);
        written.push(format!("{ga}={gb}"));
        residuals.push(format!(
            "{}-\\left({}\\right)",
            latex_of(&ga, w.names)?,
            latex_of(&gb, w.names)?
        ));
    }
    let gv: Vec<String> = unknowns.iter().map(|v| w.names.var(v)).collect();
    let command = format!("solve([{}],[{}])", written.join(","), gv.join(","));
    let mut setup = setup_lines(ctx.defs, &used, &mut names, ctx.funcs, ctx.vars);
    setup.extend(assumptions(
        ctx.conditions,
        &mut names,
        ctx.funcs,
        ctx.vars,
    )?);
    let question = if eqs.len() > 1 && split_commas(latex).len() < 2 {
        eqs.iter()
            .map(|(l, ..)| l.clone())
            .collect::<Vec<_>>()
            .join(",\\quad ")
    } else {
        latex.to_owned()
    };
    Ok(Plan {
        setup,
        command,
        op: "giac:system".into(),
        choices: Vec::new(),
        names,
        kind: Kind::System {
            vars: unknowns,
            residuals,
        },
        question,
        others: others.to_vec(),
        assumed: None,
    })
}

/// Recurrences, inequalities, matrices: the problems `plan` hands here.
pub(super) fn plan(
    statement: &Statement,
    latex: &str,
    others: &[String],
    chosen: Option<&str>,
    ctx: &Context,
) -> Option<Result<Plan, String>> {
    match statement {
        Statement::Relation(Expr::Apply(f, params), Relation::Eq, body)
            if body.mentions(f) && params.len() == 1 =>
        {
            Some(recurrence_function(f, &params[0], body, latex, others, ctx))
        }
        Statement::Relation(lhs, Relation::Eq, rhs) if subscript_key(lhs, rhs).is_some() => {
            let key = subscript_key(lhs, rhs)?;
            Some(recurrence_subscript(&key, lhs, rhs, latex, others, ctx))
        }
        Statement::Relation(Expr::Var(_) | Expr::Apply(..), Relation::Eq, _)
            if chosen != Some("system") =>
        {
            None
        }
        Statement::Relation(_, Relation::Eq, _) if chosen == Some("system") => {
            plan_system(latex, others, chosen, ctx)
        }
        Statement::Relation(a, rel, b) if *rel != Relation::Eq => {
            let c = Condition {
                operands: vec![a.clone(), b.clone()],
                relations: vec![*rel],
            };
            Some(inequality(&c, latex, others, ctx))
        }
        Statement::Chain(c) => Some(inequality(c, latex, others, ctx)),
        Statement::Expr(e) if is_matrix(e, ctx.defs) => Some(matrix(e, latex, others, chosen, ctx)),
        Statement::Expr(e) if chosen == Some("fourier") => Some(fourier(e, latex, others, ctx)),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Inequalities

fn inequality(
    c: &Condition,
    latex: &str,
    others: &[String],
    ctx: &Context,
) -> Result<Plan, String> {
    let mut names = Names::default();
    let mut used = Vec::new();
    for o in &c.operands {
        for n in free_names(o) {
            if !used.contains(&n) {
                used.push(n);
            }
        }
    }
    let unknowns: Vec<String> = used
        .iter()
        .filter(|n| !ctx.vars.contains(*n))
        .cloned()
        .collect();
    let var = main_var(&unknowns);
    let mut w = Writer {
        names: &mut names,
        funcs: ctx.funcs,
        vars: ctx.vars,
    };
    let cond = condition(c, &mut w)?;
    let gv = w.names.var(&var);
    let command = format!("solve({cond},{gv})");
    let mut setup = setup_lines(ctx.defs, &used, &mut names, ctx.funcs, ctx.vars);
    setup.extend(assumptions(
        ctx.conditions,
        &mut names,
        ctx.funcs,
        ctx.vars,
    )?);
    Ok(Plan {
        setup,
        command,
        op: "giac:solve".into(),
        choices: vec![choice(
            "giac:solve",
            &format!("Solve for ${}$", name_latex(&var)),
        )],
        names,
        kind: Kind::Inequality {
            var,
            question: latex.to_owned(),
        },
        question: latex.to_owned(),
        others: others.to_vec(),
        assumed: None,
    })
}

/// One side of an interval: the bound's LaTeX and value, and whether the
/// bound itself is in.
#[derive(Clone, Debug)]
struct End {
    latex: String,
    value: f64,
    closed: bool,
}

/// An interval, unbounded where an end is None, or a single point.
#[derive(Clone, Debug, Default)]
struct Interval {
    lo: Option<End>,
    hi: Option<End>,
    point: Option<End>,
}

/// `x > -2` or `-2 < x` in Giac's syntax, as a bound on `var`.
fn bound(atom: &str, var: &str, names: &Names, others: &[String]) -> Option<(char, End)> {
    let atom = atom
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    for (op, len) in [
        (">=", 2),
        ("<=", 2),
        ("!=", 2),
        (">", 1),
        ("<", 1),
        ("=", 1),
    ] {
        if let Some(at) = atom.find(op) {
            let (l, r) = (atom[..at].trim(), atom[at + len..].trim());
            let (value_text, flipped) = if l == var {
                (r, false)
            } else if r == var {
                (l, true)
            } else {
                return None;
            };
            let latex = latex_of(value_text, names).ok()?;
            let value = number_of(&latex, others)?;
            let mut kind = op.chars().next()?;
            if flipped {
                kind = match kind {
                    '>' => '<',
                    '<' => '>',
                    k => k,
                };
            }
            if op == "!=" {
                return None;
            }
            return Some((
                kind,
                End {
                    latex,
                    value,
                    closed: op.ends_with('=') || op == "=",
                },
            ));
        }
    }
    None
}

/// Giac's answer to an inequality as intervals, or None when it isn't one
/// this can read.
fn intervals(text: &str, var: &str, names: &Names, others: &[String]) -> Option<Vec<Interval>> {
    let t = text.trim();
    let inner = t
        .strip_prefix("list[")
        .or_else(|| t.strip_prefix('['))?
        .strip_suffix(']')?;
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for item in split_commas(inner) {
        let item = item.trim();
        if item == var {
            // Every value.
            out.push(Interval::default());
            continue;
        }
        let mut iv = Interval::default();
        for atom in item.split(" and ") {
            let (kind, end) = bound(atom, var, names, others)?;
            match kind {
                '>' => iv.lo = Some(end),
                '<' => iv.hi = Some(end),
                '=' => iv.point = Some(end),
                _ => return None,
            }
        }
        out.push(iv);
    }
    Some(out)
}

fn show_interval(iv: &Interval, v: &str) -> (String, String) {
    if let Some(p) = &iv.point {
        return (
            format!("{v}={}", p.latex),
            format!("\\left\\{{{}\\right\\}}", p.latex),
        );
    }
    let lo_rel = |e: &End| if e.closed { "\\le " } else { "<" };
    let words = match (&iv.lo, &iv.hi) {
        (Some(a), Some(b)) => format!("{}{}{v}{}{}", a.latex, lo_rel(a), lo_rel(b), b.latex),
        (Some(a), None) => format!("{v}{}{}", if a.closed { "\\ge " } else { ">" }, a.latex),
        (None, Some(b)) => format!("{v}{}{}", lo_rel(b), b.latex),
        (None, None) => format!("\\text{{every }}{v}"),
    };
    let set = format!(
        "{}{},{}{}",
        match &iv.lo {
            Some(a) if a.closed => "\\left[",
            _ => "\\left(",
        },
        iv.lo
            .as_ref()
            .map_or("-\\infty".into(), |a| a.latex.clone()),
        iv.hi.as_ref().map_or("\\infty".into(), |b| b.latex.clone()),
        match &iv.hi {
            Some(b) if b.closed => "\\right]",
            _ => "\\right)",
        },
    );
    (words, set)
}

/// The answer as words (`-2<x<2 or x>5`) and as a set in interval notation.
fn show_intervals(ivs: &[Interval], v: &str) -> (String, String) {
    let shown: Vec<(String, String)> = ivs.iter().map(|iv| show_interval(iv, v)).collect();
    (
        shown
            .iter()
            .map(|s| s.0.clone())
            .collect::<Vec<_>>()
            .join("\\quad\\text{or}\\quad "),
        shown
            .iter()
            .map(|s| s.1.clone())
            .collect::<Vec<_>>()
            .join("\\cup "),
    )
}

/// Tests one value inside each interval (which must satisfy the
/// inequality) and one in each gap (which must not).
fn inequality_check(question: &str, var: &str, ivs: &[Interval], others: &[String]) -> Check {
    let v = name_latex(var);
    let mut inside = Vec::new();
    let mut outside = Vec::new();
    let mut sorted: Vec<&Interval> = ivs.iter().filter(|iv| iv.point.is_none()).collect();
    sorted.sort_by(|a, b| {
        let key = |iv: &Interval| iv.lo.as_ref().map_or(f64::NEG_INFINITY, |e| e.value);
        key(a).total_cmp(&key(b))
    });
    for iv in &sorted {
        let p = match (&iv.lo, &iv.hi) {
            (Some(a), Some(b)) => (a.value + b.value) / 2.0,
            (Some(a), None) => a.value + 1.0,
            (None, Some(b)) => b.value - 1.0,
            (None, None) => 0.37,
        };
        inside.push(p);
    }
    for pair in sorted.windows(2) {
        if let (Some(a), Some(b)) = (&pair[0].hi, &pair[1].lo) {
            if b.value > a.value {
                outside.push((a.value + b.value) / 2.0);
            }
        }
    }
    if let Some(first) = sorted.first().and_then(|iv| iv.lo.as_ref()) {
        outside.push(first.value - 1.0);
    }
    if let Some(last) = sorted.last().and_then(|iv| iv.hi.as_ref()) {
        outside.push(last.value + 1.0);
    }
    if sorted.is_empty() {
        return Check::NotDone;
    }
    let mut rows = others.to_vec();
    rows.push(format!(
        "Q_{{chk}}\\left({v}\\right)=\\left\\{{{question}:1,0\\right\\}}"
    ));
    let base = rows.len();
    for p in inside.iter().chain(&outside) {
        rows.push(format!("Q_{{chk}}\\left({p}\\right)"));
    }
    let vals = values(&rows);
    let mut passed = 0;
    for (k, _) in inside.iter().chain(&outside).enumerate() {
        let want = if k < inside.len() { 1.0 } else { 0.0 };
        if let Some(got) = vals[base + k] {
            if got.0 != want {
                return Check::Failed;
            }
            passed += 1;
        }
    }
    if passed >= 1 {
        Check::Passed(passed)
    } else {
        Check::NotDone
    }
}

// ---------------------------------------------------------------------------
// Recurrences

/// `f(n) = {n = 0: 1, 2f(n-1) + 1}`: the initial values and the rule.
fn recurrence_function(
    f: &str,
    param: &Expr,
    body: &Expr,
    latex: &str,
    others: &[String],
    ctx: &Context,
) -> Result<Plan, String> {
    let Expr::Var(n) = param else {
        return Err("A recurrence needs a variable, like f(n)".into());
    };
    let mut funcs = ctx.funcs.clone();
    funcs.insert(f.to_owned());
    let mut names = Names::default();
    let mut w = Writer {
        names: &mut names,
        funcs: &funcs,
        vars: ctx.vars,
    };
    let gf = w.names.var(f);
    let gn = w.names.var(n);
    let (rule, starts) = match body {
        Expr::Piecewise(branches, Some(otherwise)) => {
            let mut starts = Vec::new();
            for (c, value) in branches {
                match (c.operands.as_slice(), c.relations.as_slice()) {
                    ([Expr::Var(v), k], [Relation::Eq]) | ([k, Expr::Var(v)], [Relation::Eq])
                        if v == n && free_names(k).is_empty() =>
                    {
                        starts.push((w.expr(k)?, w.expr(value)?));
                    }
                    _ => {
                        return Err(
                            "Write each starting value as n = k, like {n = 0: 1, 2f(n-1)}".into(),
                        )
                    }
                }
            }
            (w.expr(otherwise)?, starts)
        }
        Expr::Piecewise(..) => {
            return Err(
                "Give the rule for other n as the last branch, like {n = 0: 1, 2f(n-1)}".into(),
            )
        }
        e => (w.expr(e)?, Vec::new()),
    };
    let mut command = format!("rsolve({gf}({gn})={rule},{gf}({gn})");
    for (k, v) in &starts {
        command.push_str(&format!(",{gf}({k})={v}"));
    }
    command.push(')');
    let mut used = free_names(body);
    used.retain(|u| u != n);
    let mut setup = setup_lines(ctx.defs, &used, &mut names, ctx.funcs, ctx.vars);
    setup.extend(assumptions(
        ctx.conditions,
        &mut names,
        ctx.funcs,
        ctx.vars,
    )?);
    let first = starts
        .iter()
        .filter_map(|(k, _)| k.trim_matches(|c| c == '(' || c == ')').parse::<i64>().ok())
        .max();
    let shown_f = name_latex(f);
    let shown_n = name_latex(n);
    Ok(Plan {
        setup,
        command,
        op: "giac:rsolve".into(),
        choices: vec![choice("giac:rsolve", "Closed form")],
        names,
        kind: Kind::Recurrence {
            call: format!("{shown_f}\\left({shown_n}\\right)"),
            var: n.clone(),
            check: first.map(|k| RecurrenceCheck::Function {
                definition: latex.to_owned(),
                name: shown_f,
                from: k,
            }),
        },
        question: latex.to_owned(),
        others: others.to_vec(),
        assumed: None,
    })
}

/// The letter of a subscript recurrence like `a_{n+1} = 2a_n + 1`.
fn subscript_key(lhs: &Expr, rhs: &Expr) -> Option<String> {
    let key = match lhs {
        Expr::Apply(k, args) if k.ends_with('_') && args.len() == 1 => k.clone(),
        Expr::Var(name) => crate::subscripts::indexed(name)?.0,
        _ => return None,
    };
    let mut found = false;
    rhs.visit_names(&mut |n, _| {
        found |= n == key || n.strip_prefix(key.as_str()).is_some_and(|s| !s.is_empty());
    });
    found.then_some(key)
}

/// `a_n`, `a_{n-1}` and `a_0` as calls `a_(n)`, so Giac sees one sequence.
fn as_calls(e: &Expr, key: &str) -> Expr {
    crate::subscripts::rewrite(e.clone(), &|_| false, &|k| k == key)
}

fn recurrence_subscript(
    key: &str,
    lhs: &Expr,
    rhs: &Expr,
    latex: &str,
    others: &[String],
    ctx: &Context,
) -> Result<Plan, String> {
    let lhs = as_calls(lhs, key);
    let rhs = as_calls(rhs, key);
    let Expr::Apply(_, args) = &lhs else {
        return Err(
            "Write the recurrence with the sequence on the left, like a_{n+1} = 2a_n".into(),
        );
    };
    let n = free_names(&args[0])
        .into_iter()
        .next()
        .ok_or("The subscript needs a variable, like a_{n+1}")?;
    let mut funcs = ctx.funcs.clone();
    funcs.insert(key.to_owned());
    let mut names = Names::default();
    let mut w = Writer {
        names: &mut names,
        funcs: &funcs,
        vars: ctx.vars,
    };
    let ga = w.names.var(key);
    let gn = w.names.var(&n);
    let mut command = format!("rsolve({}={},{ga}({gn})", w.expr(&lhs)?, w.expr(&rhs)?);
    // Starting values from other rows: a_0 = 1.
    let mut starts = Vec::new();
    for d in ctx.defs {
        if let Statement::Relation(Expr::Var(name), Relation::Eq, value) = d {
            if let Some(Expr::Apply(k, at)) = crate::subscripts::as_call(name, &|k| k == key) {
                if k == key && free_names(&at[0]).is_empty() && free_names(value).is_empty() {
                    let (gk, gv) = (w.expr(&at[0])?, w.expr(value)?);
                    command.push_str(&format!(",{ga}({gk})={gv}"));
                    starts.push(name.clone());
                }
            }
        }
    }
    command.push(')');
    let setup = vec![
        "restart".to_owned(),
        format!("angle_radian:={}", if degrees() { 0 } else { 1 }),
    ];
    let letter = key.trim_end_matches('_');
    Ok(Plan {
        setup,
        command,
        op: "giac:rsolve".into(),
        choices: vec![choice("giac:rsolve", "Closed form")],
        names,
        kind: Kind::Recurrence {
            call: format!("{}_{{{}}}", name_latex(letter), name_latex(&n)),
            var: n.clone(),
            check: Some(RecurrenceCheck::Subscript {
                rule: latex.to_owned(),
                letter: letter.to_owned(),
                var: n,
                starts,
            }),
        },
        question: latex.to_owned(),
        others: others.to_vec(),
        assumed: None,
    })
}

/// How a closed form is checked against the recurrence it came from.
#[derive(Clone, Debug)]
pub(super) enum RecurrenceCheck {
    /// The engine works the recursive function out (it can call itself),
    /// for n from `from` + 1 on.
    Function {
        definition: String,
        name: String,
        from: i64,
    },
    /// The closed form, put in place of the sequence, satisfies the rule
    /// and the starting values.
    Subscript {
        rule: String,
        letter: String,
        var: String,
        starts: Vec<String>,
    },
}

/// `a_{n+1}`, `a_n` and `a_0` in LaTeX as calls of `C_{chk}`.
fn replace_sequence(latex: &str, letter: &str) -> String {
    let mut out = String::new();
    let mut rest = latex;
    let pat = format!("{letter}_");
    while let Some(at) = rest.find(&pat) {
        // Only the letter on its own, not the end of a longer name.
        let before_ok = rest[..at]
            .chars()
            .last()
            .is_none_or(|c| !c.is_ascii_alphabetic());
        out.push_str(&rest[..at]);
        let after = &rest[at + pat.len()..];
        if !before_ok {
            out.push_str(&pat);
            rest = after;
            continue;
        }
        let (sub, tail) = if let Some(inner) = after.strip_prefix('{') {
            let mut depth = 1;
            let mut end = inner.len();
            for (i, c) in inner.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            (&inner[..end], inner.get(end + 1..).unwrap_or(""))
        } else {
            let len = after.chars().next().map_or(0, char::len_utf8);
            (&after[..len], &after[len..])
        };
        out.push_str(&format!("C_{{chk}}\\left({sub}\\right)"));
        rest = tail;
    }
    out.push_str(rest);
    out
}

pub(super) fn recurrence_check(
    check: &RecurrenceCheck,
    closed: &str,
    var: &str,
    others: &[String],
) -> Check {
    let v = name_latex(var);
    let mut rows = others.to_vec();
    rows.push(format!("C_{{chk}}\\left({v}\\right)={closed}"));
    match check {
        RecurrenceCheck::Function {
            definition,
            name,
            from,
        } => {
            rows.push(definition.clone());
            let base = rows.len();
            let at: Vec<i64> = (from + 1..from + 7).collect();
            for k in &at {
                rows.push(format!("{name}\\left({k}\\right)"));
                rows.push(format!("C_{{chk}}\\left({k}\\right)"));
            }
            let vals = values(&rows);
            let mut passed = 0;
            for k in 0..at.len() {
                if let (Some(a), Some(b)) = (vals[base + 2 * k], vals[base + 2 * k + 1]) {
                    if !close(a, b, 1e-6) {
                        return Check::Failed;
                    }
                    passed += 1;
                }
            }
            if passed >= 2 {
                Check::Passed(passed)
            } else {
                Check::NotDone
            }
        }
        RecurrenceCheck::Subscript {
            rule,
            letter,
            var,
            starts,
        } => {
            let Some((l, r)) = replace_sequence(rule, letter)
                .split_once('=')
                .map(|(a, b)| (a.to_owned(), b.to_owned()))
            else {
                return Check::NotDone;
            };
            let v = name_latex(var);
            rows.push(format!(
                "R_{{chk}}\\left({v}\\right)={l}-\\left({r}\\right)"
            ));
            let base = rows.len();
            for k in 1..7 {
                rows.push(format!("R_{{chk}}\\left({k}\\right)"));
            }
            let mut start_rows = Vec::new();
            for s in starts {
                // a_0 = 1 in another row: C(0) must be 1 too.
                if let Some(row) = others.iter().find(|o| {
                    o.split_once('=')
                        .is_some_and(|(lhs, _)| lhs.trim().replace(['{', '}'], "") == s.as_str())
                }) {
                    let (lhs, rhs) = row.split_once('=').unwrap_or((row, "0"));
                    start_rows.push(format!(
                        "{}-\\left({rhs}\\right)",
                        replace_sequence(lhs, letter)
                    ));
                }
            }
            rows.extend(start_rows.iter().cloned());
            // The other rows define a_0 and so on, which the check mustn't
            // read as the sequence itself; they are only needed above.
            let vals = values(&rows);
            let mut passed = 0;
            for k in 0..6 + start_rows.len() {
                if let Some(a) = vals[base + k] {
                    if !close(a, (0.0, 0.0), 1e-6) {
                        return Check::Failed;
                    }
                    passed += 1;
                }
            }
            if passed >= 2 {
                Check::Passed(passed)
            } else {
                Check::NotDone
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Matrices

/// Whether `e` is a matrix: written out, a name whose row is one, or
/// arithmetic on them.
fn is_matrix(e: &Expr, defs: &[Statement]) -> bool {
    match e {
        Expr::List(rows) => !rows.is_empty() && rows.iter().all(|r| matches!(r, Expr::List(_))),
        Expr::Var(n) => defs.iter().any(|d| {
            matches!(d, Statement::Relation(Expr::Var(m), Relation::Eq, v) if m == n && is_matrix(v, &[]))
        }),
        Expr::Add(a, b) | Expr::Sub(a, b) => is_matrix(a, defs) || is_matrix(b, defs),
        Expr::Mul(a, b) => is_matrix(a, defs) || is_matrix(b, defs),
        Expr::Pow(a, _) | Expr::Neg(a) => is_matrix(a, defs),
        Expr::Apply(f, args) => {
            matches!(f.as_str(), "inv" | "transpose" | "rref" | "identity")
                && args.iter().all(|a| is_matrix(a, defs) || f == "identity")
        }
        _ => false,
    }
}

const MATRIX_OPS: &[(&str, &str)] = &[
    ("matrix", "Exact value"),
    ("eigenvals", "Eigenvalues"),
    ("eigenvects", "Eigenvectors"),
    ("inv", "Inverse"),
    ("det", "Determinant"),
    ("rref", "Row reduce"),
    ("charpoly", "Characteristic polynomial"),
];

fn matrix(
    e: &Expr,
    latex: &str,
    others: &[String],
    chosen: Option<&str>,
    ctx: &Context,
) -> Result<Plan, String> {
    let what = chosen
        .filter(|c| MATRIX_OPS.iter().any(|(op, _)| op == c))
        .unwrap_or("matrix")
        .to_owned();
    let mut names = Names::default();
    let used = free_names(e);
    let mut w = Writer {
        names: &mut names,
        funcs: ctx.funcs,
        vars: ctx.vars,
    };
    let g = w.expr(e)?;
    let lambda = w.names.var("lambda");
    let command = match what.as_str() {
        "eigenvals" => format!("[eigenvals({g})]"),
        "eigenvects" => format!("[egvl({g}),egv({g})]"),
        "inv" => format!("simplify(inv({g}))"),
        "det" => format!("simplify(det({g}))"),
        "rref" => format!("rref({g})"),
        "charpoly" => format!("factor(det({g}-{lambda}*idn(rowdim({g}))))"),
        _ => format!("simplify({g})"),
    };
    let mut setup = setup_lines(ctx.defs, &used, &mut names, ctx.funcs, ctx.vars);
    setup.extend(assumptions(
        ctx.conditions,
        &mut names,
        ctx.funcs,
        ctx.vars,
    )?);
    Ok(Plan {
        setup,
        command,
        op: format!("giac:{what}"),
        choices: MATRIX_OPS
            .iter()
            .map(|(op, label)| choice(&format!("giac:{op}"), label))
            .collect(),
        names,
        kind: Kind::Matrix {
            what,
            original: latex.to_owned(),
        },
        question: latex.to_owned(),
        others: others.to_vec(),
        assumed: None,
    })
}

/// A Giac matrix (a list of equal rows) in LaTeX.
pub(super) fn matrix_latex(g: &G, p: &Printer) -> Option<String> {
    let G::List(rows) = g else { return None };
    let mut out = Vec::new();
    let mut width = None;
    for r in rows {
        let G::List(cells) = r else { return None };
        if *width.get_or_insert(cells.len()) != cells.len() {
            return None;
        }
        let cells: Vec<String> = cells
            .iter()
            .map(|c| p.latex(c).ok())
            .collect::<Option<_>>()?;
        out.push(cells.join("&"));
    }
    Some(format!(
        "\\begin{{bmatrix}}{}\\end{{bmatrix}}",
        out.join("\\\\")
    ))
}

/// The numbers in a matrix of LaTeX entries.
fn numeric_matrix(g: &G, p: &Printer, others: &[String]) -> Option<crate::Matrix> {
    let G::List(rows) = g else { return None };
    let mut data = Vec::new();
    let mut width = 0;
    for r in rows {
        let G::List(cells) = r else { return None };
        width = cells.len();
        for c in cells {
            data.push(number_of(&p.latex(c).ok()?, others)?);
        }
    }
    (width > 0).then(|| crate::Matrix::new(rows.len(), width, data))
}

/// The engine's own value of `latex` as a matrix, list or number.
fn engine_value(latex: &str, others: &[String]) -> Option<(Option<crate::Matrix>, Vec<f64>)> {
    let mut rows = others.to_vec();
    rows.push(latex.to_owned());
    let out = analyze_rows(&rows);
    let last = out.last()?;
    let list = match &last.value {
        Some(Value::Number(v)) => vec![*v],
        Some(Value::Numbers(v)) => v.clone(),
        _ => Vec::new(),
    };
    Some((last.matrix.clone(), list))
}

fn same_matrix(a: &crate::Matrix, b: &crate::Matrix) -> bool {
    a.rows == b.rows
        && a.cols == b.cols
        && a.data
            .iter()
            .zip(&b.data)
            .all(|(x, y)| (x - y).abs() <= 1e-6 * (1.0 + y.abs()))
}

/// Giac's matrix answer checked against the engine's numeric one.
pub(super) fn matrix_check(
    what: &str,
    original: &str,
    answer: &G,
    p: &Printer,
    others: &[String],
) -> Check {
    let call = |f: &str| format!("\\operatorname{{{f}}}\\left({original}\\right)");
    match what {
        "matrix" | "inv" | "rref" => {
            let question = match what {
                "inv" => call("inv"),
                "rref" => call("rref"),
                _ => original.to_owned(),
            };
            let (Some(ours), Some((Some(theirs), _))) = (
                numeric_matrix(answer, p, others),
                engine_value(&question, others),
            ) else {
                return Check::NotDone;
            };
            if same_matrix(&ours, &theirs) {
                Check::Passed(1)
            } else {
                Check::Failed
            }
        }
        "det" => match p.latex(answer) {
            Ok(l) => value_check(&format!("\\det\\left({original}\\right)"), &l, others, 1e-6),
            Err(_) => Check::NotDone,
        },
        "eigenvals" => {
            let G::List(items) = answer else {
                return Check::NotDone;
            };
            let mut ours: Vec<f64> = Vec::new();
            for i in items {
                match p.latex(i).ok().and_then(|l| number_of(&l, others)) {
                    Some(v) => ours.push(v),
                    // Complex eigenvalues: the engine lists real ones only.
                    None => return Check::NotDone,
                }
            }
            let Some((_, mut theirs)) = engine_value(&call("eigenvalues"), others) else {
                return Check::NotDone;
            };
            ours.sort_by(f64::total_cmp);
            theirs.sort_by(f64::total_cmp);
            ours.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            theirs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
            if ours.len() != theirs.len() || theirs.is_empty() {
                return Check::NotDone;
            }
            if ours
                .iter()
                .zip(&theirs)
                .all(|(a, b)| (a - b).abs() <= 1e-6 * (1.0 + b.abs()))
            {
                Check::Passed(ours.len())
            } else {
                Check::Failed
            }
        }
        "eigenvects" => {
            // A v = λ v for each column v of P, λ from D's diagonal.
            let G::List(pair) = answer else {
                return Check::NotDone;
            };
            let (Some(d), Some(pm), Some((Some(a), _))) = (
                pair.first().and_then(|g| numeric_matrix(g, p, others)),
                pair.get(1).and_then(|g| numeric_matrix(g, p, others)),
                engine_value(original, others),
            ) else {
                return Check::NotDone;
            };
            if a.rows != pm.rows || a.cols != pm.rows {
                return Check::NotDone;
            }
            for col in 0..pm.cols {
                let lambda = d.get(col, col);
                for r in 0..a.rows {
                    let av: f64 = (0..a.cols).map(|k| a.get(r, k) * pm.get(k, col)).sum();
                    let lv = lambda * pm.get(r, col);
                    if (av - lv).abs() > 1e-6 * (1.0 + lv.abs()) {
                        return Check::Failed;
                    }
                }
            }
            Check::Passed(pm.cols)
        }
        _ => Check::NotDone,
    }
}

// ---------------------------------------------------------------------------
// Fourier transforms

fn fourier(e: &Expr, latex: &str, others: &[String], ctx: &Context) -> Result<Plan, String> {
    let used = free_names(e);
    let var = main_var(&used);
    let mut names = Names::default();
    let mut w = Writer {
        names: &mut names,
        funcs: ctx.funcs,
        vars: ctx.vars,
    };
    let g = w.expr(e)?;
    let gv = w.names.var(&var);
    let omega = w.names.var("omega");
    // F(ω) = ∫ f(x) e^{-iωx} dx over the whole line, as an integral Giac
    // can do; its own fourier() leaves most transforms unevaluated.
    let command =
        format!("simplify(integrate(({g})*exp(-i*{omega}*{gv}),{gv},-infinity,+infinity))");
    let mut setup = setup_lines(ctx.defs, &used, &mut names, ctx.funcs, ctx.vars);
    setup.extend(assumptions(
        ctx.conditions,
        &mut names,
        ctx.funcs,
        ctx.vars,
    )?);
    let v = name_latex(&var);
    Ok(Plan {
        setup,
        command,
        op: "giac:fourier".into(),
        choices: Vec::new(),
        names,
        kind: Kind::Fourier {
            var,
            original: latex.to_owned(),
        },
        question: format!(
            "\\int_{{-\\infty}}^{{\\infty}}\\left({latex}\\right)e^{{-i\\omega {v}}}d{v}"
        ),
        others: others.to_vec(),
        assumed: None,
    })
}

/// F(ω) checked at a few ω against the engine's numeric integral of
/// f(x) cos(ωx) and f(x) sin(ωx).
pub(super) fn fourier_check(f: &str, var: &str, big: &str, others: &[String]) -> Check {
    let v = name_latex(var);
    let mut rows = others.to_vec();
    rows.push(format!("F_{{chk}}\\left({v}\\right)={f}"));
    rows.push(format!("G_{{chk}}\\left(\\omega\\right)={big}"));
    let ws = [0.5, 1.3];
    let base = rows.len();
    for w in ws {
        rows.push(format!(
            "\\int_{{-\\infty}}^{{\\infty}}F_{{chk}}\\left(t\\right)\\cos\\left({w}t\\right)dt"
        ));
        rows.push(format!(
            "-\\int_{{-\\infty}}^{{\\infty}}F_{{chk}}\\left(t\\right)\\sin\\left({w}t\\right)dt"
        ));
        rows.push(format!("G_{{chk}}\\left({w}\\right)"));
    }
    let vals = values(&rows);
    let mut passed = 0;
    for k in 0..ws.len() {
        if let (Some(re), Some(im), Some(g)) = (
            vals[base + 3 * k],
            vals[base + 3 * k + 1],
            vals[base + 3 * k + 2],
        ) {
            if !close((re.0, im.0), g, 1e-4) {
                return Check::Failed;
            }
            passed += 1;
        }
    }
    if passed >= 1 {
        Check::Passed(passed)
    } else {
        Check::NotDone
    }
}

// ---------------------------------------------------------------------------
// Answers

fn step(note: impl Into<String>, latex: impl Into<String>) -> Step {
    Step {
        note: note.into(),
        latex: latex.into(),
    }
}

/// Writes out Giac's answer for the kinds of problem planned here, and
/// checks it: None when the report is finished as it is.
pub(super) fn finish(
    plan: &Plan,
    text: &str,
    shown: &str,
    readable: bool,
    report: &mut Report,
) -> Option<Check> {
    let p = Printer { names: &plan.names };
    let others = &plan.others;
    let q = &plan.question;
    match &plan.kind {
        Kind::System { vars, residuals } => {
            let sols: Vec<Vec<String>> = match read_giac(text) {
                Ok(G::List(items)) => items
                    .iter()
                    .filter_map(|s| match s {
                        G::List(vals) if vals.len() == vars.len() => {
                            vals.iter().map(|v| p.latex(v).ok()).collect()
                        }
                        // One unknown: a plain list of values.
                        v if vars.len() == 1 => p.latex(v).ok().map(|l| vec![l]),
                        _ => None,
                    })
                    .collect(),
                _ => {
                    report.error = Some(format!("Giac's answer couldn't be read: {shown}"));
                    return None;
                }
            };
            report
                .steps
                .push(step("Solve the equations together", q.clone()));
            if sols.is_empty() {
                report
                    .steps
                    .push(step("Giac finds no values that satisfy them all", ""));
                report.result = Some("\\text{No solutions}".into());
                return None;
            }
            let shown_vars: Vec<String> = vars.iter().map(|v| name_latex(v)).collect();
            let each: Vec<String> = sols
                .iter()
                .map(|s| {
                    shown_vars
                        .iter()
                        .zip(s)
                        .map(|(v, x)| format!("{v}={x}"))
                        .collect::<Vec<_>>()
                        .join(",\\ ")
                })
                .collect();
            let joined = each.join("\\quad\\text{or}\\quad ");
            report.steps.push(step(
                if sols.len() == 1 {
                    "Giac solves them".to_owned()
                } else {
                    format!("Giac finds {} solutions", sols.len())
                },
                joined.clone(),
            ));
            report.result = Some(joined);
            // In x and y, the solutions are where the graphs cross.
            if vars.len() == 2 && vars[0] == "x" && vars[1] == "y" {
                report.answer = Some(format!(
                    "\\left[{}\\right]",
                    sols.iter()
                        .map(|s| format!("\\left({},{}\\right)", s[0], s[1]))
                        .collect::<Vec<_>>()
                        .join(",")
                ));
            }
            // Each solution put back into each equation.
            let args = shown_vars.join(",");
            let mut rows = others.clone();
            for (k, r) in residuals.iter().enumerate() {
                rows.push(format!("R_{{chk{k}}}\\left({args}\\right)={r}"));
            }
            let base = rows.len();
            for s in &sols {
                for k in 0..residuals.len() {
                    rows.push(format!("R_{{chk{k}}}\\left({}\\right)", s.join(",")));
                }
            }
            let vals = values(&rows);
            let mut passed = 0;
            for v in vals[base..].iter().flatten() {
                if !close(*v, (0.0, 0.0), 1e-6) {
                    return Some(Check::Failed);
                }
                passed += 1;
            }
            Some(if passed > 0 && passed == vals.len() - base {
                Check::Passed(sols.len())
            } else {
                Check::NotDone
            })
        }
        Kind::Inequality { var, question } => {
            let v = name_latex(var);
            report
                .steps
                .push(step(format!("Solve for ${v}$"), q.clone()));
            let Some(ivs) = intervals(text, &plan.names.var_back(var), &plan.names, others) else {
                report.steps.push(step("Giac solves it", shown.to_owned()));
                report.result = Some(shown.to_owned());
                return Some(Check::NotDone);
            };
            if ivs.is_empty() {
                report
                    .steps
                    .push(step(format!("No value of ${v}$ satisfies it"), ""));
                report.result = Some(format!("\\text{{No solutions for }}{v}"));
                return None;
            }
            let (words, set) = show_intervals(&ivs, &v);
            report.steps.push(step("Giac solves it", words.clone()));
            report
                .steps
                .push(step("In interval notation", format!("{v}\\in {set}")));
            report.result = Some(words);
            Some(inequality_check(question, var, &ivs, others))
        }
        Kind::Recurrence { call, var, check } => {
            let closed = match read_giac(text) {
                Ok(G::List(items)) if items.len() == 1 => p.latex(&items[0]).ok(),
                Ok(G::List(_)) => None,
                Ok(g) => p.latex(&g).ok(),
                Err(_) => None,
            };
            let Some(closed) = closed.filter(|_| readable && !text.contains("rsolve")) else {
                report.error = Some("Giac found no closed form for this recurrence".into());
                return None;
            };
            report
                .steps
                .push(step("Find a formula for the recurrence", q.clone()));
            report.steps.push(step(
                if closed.contains("c_{") {
                    "Giac solves the recurrence; $c_{0}$, $c_{1}$, … are constants that starting values would fix"
                } else {
                    "Giac solves the recurrence from its starting values"
                },
                format!("{call}={closed}"),
            ));
            report.result = Some(format!("{call}={closed}"));
            Some(match check {
                Some(c) if !closed.contains("c_{") => recurrence_check(c, &closed, var, others),
                _ => Check::NotDone,
            })
        }
        Kind::Matrix { what, original } => {
            let g = match read_giac(text) {
                Ok(g) => g,
                Err(_) => {
                    report
                        .steps
                        .push(step("Giac works it out", format!("{q}={shown}")));
                    report.result = Some(shown.to_owned());
                    return Some(Check::NotDone);
                }
            };
            let shown = matrix_latex(&g, &p)
                .or_else(|| p.latex(&g).ok())
                .unwrap_or_else(|| shown.to_owned());
            let call = |f: &str| format!("\\operatorname{{{f}}}\\left({q}\\right)");
            match what.as_str() {
                "eigenvals" => {
                    let G::List(items) = &g else {
                        report.error = Some("Giac's eigenvalues couldn't be read".into());
                        return None;
                    };
                    let list: Vec<String> = items.iter().filter_map(|i| p.latex(i).ok()).collect();
                    report.steps.push(step(
                        "The eigenvalues are the roots of $\\det(A-\\lambda I)=0$",
                        format!("\\lambda={}", list.join(",\\ ")),
                    ));
                    report.result = Some(format!("\\lambda={}", list.join(",\\ ")));
                    report.answer = Some(format!("\\left[{}\\right]", list.join(",")));
                }
                "eigenvects" => {
                    let G::List(pair) = &g else {
                        report.error = Some("Giac's eigenvectors couldn't be read".into());
                        return None;
                    };
                    let (Some(d), Some(pm)) = (
                        pair.first().and_then(|m| matrix_latex(m, &p)),
                        pair.get(1).and_then(|m| matrix_latex(m, &p)),
                    ) else {
                        report.error = Some("Giac's eigenvectors couldn't be read".into());
                        return None;
                    };
                    report.steps.push(step(
                        "Each column of $P$ is an eigenvector, for the eigenvalue in the same column of $D$",
                        format!("P={pm},\\quad D={d}"),
                    ));
                    report.steps.push(step(
                        "So the matrix is diagonalized as",
                        format!("{q}=PDP^{{-1}}"),
                    ));
                    report.result = Some(format!("P={pm},\\quad D={d}"));
                }
                "charpoly" => {
                    report.steps.push(step(
                        "The characteristic polynomial is $\\det(A-\\lambda I)$",
                        format!("\\det\\left({q}-\\lambda I\\right)={shown}"),
                    ));
                    report.result = Some(shown.clone());
                }
                other => {
                    let lhs = match other {
                        "inv" => format!("\\left({q}\\right)^{{-1}}"),
                        "det" => format!("\\det\\left({q}\\right)"),
                        "rref" => call("rref"),
                        _ => q.clone(),
                    };
                    report
                        .steps
                        .push(step("Giac works it out exactly", format!("{lhs}={shown}")));
                    report.result = Some(shown.clone());
                    report.answer = Some(shown.clone());
                }
            }
            Some(matrix_check(what, original, &g, &p, others))
        }
        Kind::Fourier { var, original } => {
            if text.contains("integrate(") || text.contains("undef") {
                report.error = Some("Giac couldn't find this Fourier transform exactly".into());
                return None;
            }
            report.steps.push(step(
                "The Fourier transform $F(\\omega)$ is this integral",
                q.clone(),
            ));
            report.steps.push(step(
                "Giac works it out",
                format!("F\\left(\\omega\\right)={shown}"),
            ));
            report.result = Some(format!("F\\left(\\omega\\right)={shown}"));
            Some(if readable {
                fourier_check(original, var, shown, others)
            } else {
                Check::NotDone
            })
        }
        Kind::Nicest { .. } => {
            let Some((form, check)) = nicest(plan, text) else {
                report.error = Some("Giac found no other form that checks out".into());
                return None;
            };
            report.steps.push(step(
                "Giac tries several forms and keeps the tidiest",
                format!("{q}={form}"),
            ));
            report.result = Some(form.clone());
            report.answer = Some(form);
            Some(check)
        }
        _ => Some(Check::NotDone),
    }
}

/// The tidiest of Giac's equivalent forms that passes the numeric check:
/// its LaTeX, or None when none does.
pub(super) fn nicest(plan: &Plan, text: &str) -> Option<(String, Check)> {
    let Kind::Nicest { var, original } = &plan.kind else {
        return None;
    };
    let p = Printer { names: &plan.names };
    let Ok(G::List(items)) = read_giac(text) else {
        return None;
    };
    let mut forms: Vec<String> = items.iter().filter_map(|g| p.latex(g).ok()).collect();
    forms.sort_by_key(|f| f.len());
    forms.dedup();
    for f in forms {
        let check = match var {
            Some(v) => same_function(original, &f, v, &plan.others),
            None => value_check(original, &f, &plan.others, 1e-9),
        };
        if matches!(check, Check::Passed(_)) {
            return Some((f, check));
        }
    }
    None
}
