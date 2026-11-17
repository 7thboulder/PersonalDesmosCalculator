//! Solve with sets: answers kept to a set (`x^2 = 4, x ∈ ℕ`, or a row of its
//! own saying `x ∈ ℕ`), whole-number (Diophantine) equations, and ∀ and ∃
//! statements proved where the algebra allows, checked numerically where
//! it doesn't.

use std::cell::RefCell;

use super::convert::{to_sym, Defs};
use super::latex::{latex, name};
use super::solve::{self, Outcome};
use super::sym::*;
use super::{nice, Report, Steps};
use crate::ast::{Expr, Relation};
use crate::logic::{Domain, Prop, Quant, SetExpr};

/// A variable kept to a set, with the set as typed.
#[derive(Clone, Debug)]
pub struct VarDomain {
    pub var: String,
    pub domain: Domain,
    /// `x\in\mathbb{N}`, for the working.
    pub latex: String,
}

thread_local! {
    static DOMAINS: RefCell<Vec<VarDomain>> = const { RefCell::new(Vec::new()) };
}

pub fn set_domains(domains: Vec<VarDomain>) {
    DOMAINS.with(|d| *d.borrow_mut() = domains);
}

pub fn domain_of(v: &str) -> Option<VarDomain> {
    DOMAINS.with(|d| d.borrow().iter().find(|x| x.var == v).cloned())
}

/// Splits `latex` at its top-level commas (outside brackets of any kind).
fn split_top(latex: &str) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut depth = 0i32;
    let chars: Vec<char> = latex.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let rest: String = chars[i..chars.len().min(i + 7)].iter().collect();
        if rest.starts_with("\\left") {
            depth += 1;
        } else if rest.starts_with("\\right") {
            depth -= 1;
        }
        match c {
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(String::new());
                i += 1;
                continue;
            }
            _ => {}
        }
        parts.last_mut().expect("one part").push(c);
        i += 1;
    }
    parts
}

/// The numbers an expression in a set stands for: a number, or a list of
/// numbers (`[1, 2, 3]`, `[1...10]`).
fn numbers(e: &Expr, defs: &Defs) -> Result<Vec<f64>, String> {
    let value = |e: &Expr| -> Result<f64, String> {
        let v = match e {
            Expr::Var(n) if n == "infty" => f64::INFINITY,
            Expr::Neg(inner) if matches!(&**inner, Expr::Var(n) if n == "infty") => {
                f64::NEG_INFINITY
            }
            _ => to_sym(e, defs)?.value(),
        };
        if v.is_nan() {
            Err("A set for Solve must be made of numbers".into())
        } else {
            Ok(v)
        }
    };
    Ok(match e {
        Expr::List(items) => items.iter().map(value).collect::<Result<_, _>>()?,
        Expr::Range(a, None, b) => {
            let (a, b) = (value(a)?, value(b)?);
            if !(b - a).is_finite() || (b - a).abs() > 1e5 {
                return Err("That list is too long".into());
            }
            let mut out = Vec::new();
            let mut x = a;
            while x <= b + 1e-9 {
                out.push(x);
                x += 1.0;
            }
            out
        }
        other => vec![value(other)?],
    })
}

/// `x \in S` (or `x, y \in S`) as typed: the variables and the set.
fn membership(latex: &str) -> Option<(Vec<String>, SetExpr)> {
    match crate::parser::parse_statement(latex)? {
        Ok(Prop::Member(Expr::Var(v), set, false)) => Some((vec![v], set)),
        _ => None,
    }
}

/// The equation (or other row) without its `, x ∈ S`, and the sets its
/// variables are kept to, from the end of the row or from other rows that
/// say only `x ∈ S`.
pub fn split(
    source: &str,
    others: &[String],
    defs: &Defs,
) -> Result<(String, Vec<VarDomain>), String> {
    let mut parts = split_top(source);
    let mut found: Vec<VarDomain> = Vec::new();
    let mut add = |vars: Vec<String>, set: &SetExpr, typed: String| -> Result<(), String> {
        let domain = Domain::from_set(set, &mut |e| numbers(e, defs))?;
        for v in vars {
            if !found.iter().any(|d| d.var == v) {
                found.push(VarDomain {
                    var: v,
                    domain: domain.clone(),
                    latex: typed.clone(),
                });
            }
        }
        Ok(())
    };
    while parts.len() > 1 {
        let last = parts.last().expect("parts").trim().to_owned();
        let Some((mut vars, set)) = membership(&last) else {
            break;
        };
        parts.pop();
        // `x, y \in \mathbb{Z}`: bare letters before it share the set.
        while parts.len() > 1 {
            let prev = parts.last().expect("parts").trim().to_owned();
            match crate::parse(&prev) {
                Ok(crate::ast::Statement::Expr(Expr::Var(v))) => {
                    vars.insert(0, v);
                    parts.pop();
                }
                _ => break,
            }
        }
        let typed = if vars.len() > 1 {
            let names: Vec<String> = vars.iter().map(|v| name(v)).collect();
            let set_part = last.split_once("\\in").map_or(last.as_str(), |(_, s)| s);
            format!("{}\\in {}", names.join(","), set_part.trim())
        } else {
            last.clone()
        };
        add(vars, &set, typed)?;
    }
    for row in others {
        if let Some((vars, set)) = membership(row.trim()) {
            add(vars, &set, row.trim().to_owned())?;
        }
    }
    Ok((parts.join(","), found))
}

/// Keeps the roots in `v`'s set, saying which go. Roots in terms of an
/// integer k (as sin x = 0 has) become the ones inside a bounded set.
pub fn restrict(roots: Vec<Sym>, v: &str, st: &mut Steps) -> Vec<Sym> {
    let Some(d) = domain_of(v) else {
        return roots;
    };
    let xs = name(v);
    let k = solve::integer_name(v);
    let mut kept: Vec<Sym> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();
    for root in roots {
        if root.has(k) {
            match d.domain.bounds() {
                Some((lo, hi)) => {
                    for j in -2000..=2000 {
                        let s = nice(&root.subs_var(k, &n(j)));
                        if !s.is_constant() {
                            continue;
                        }
                        let t = s.value();
                        if t >= lo - 1e-9
                            && t <= hi + 1e-9
                            && d.domain.contains(t).unwrap_or(false)
                            && !kept
                                .iter()
                                .any(|w| w.is_constant() && (w.value() - t).abs() < 1e-9)
                        {
                            kept.push(s);
                        }
                    }
                }
                None => kept.push(root),
            }
            continue;
        }
        if !root.is_constant() {
            kept.push(root);
            continue;
        }
        match d.domain.contains(root.value()) {
            Ok(false) => dropped.push(latex(&root)),
            _ => kept.push(root),
        }
    }
    if !dropped.is_empty() {
        st.push(
            format!(
                "Keep only ${}$: drop ${xs}={}$",
                d.latex,
                dropped.join(",\\ ")
            ),
            "",
        );
    } else {
        st.push(format!("Every answer here has ${}$", d.latex), "");
    }
    if kept.iter().all(Sym::is_constant) {
        kept.sort_by(|a, b| a.value().total_cmp(&b.value()));
    }
    kept
}

type Done = Result<(String, Option<String>), String>;

fn gcd_ext(a: i128, b: i128) -> (i128, i128, i128) {
    if b == 0 {
        (a.abs(), a.signum(), 0)
    } else {
        let (g, x, y) = gcd_ext(b, a % b);
        (g, y, x - (a / b) * y)
    }
}

/// An equation in two whole-number unknowns. A linear one, ax + by = c, is
/// solved exactly with Euclid's algorithm; others are searched.
/// c·v written out: v, -v or 3v.
fn times(c: i128, v: &str) -> String {
    match c {
        1 => v.to_owned(),
        -1 => format!("-{v}"),
        _ => format!("{c}{v}"),
    }
}

/// c·v after another term: +v, -v, +3v or -3v.
fn signed(c: i128, v: &str) -> String {
    if c < 0 {
        times(c, v)
    } else {
        format!("+{}", times(c, v))
    }
}

pub fn diophantine(l: &Sym, r: &Sym, vars: &[String], st: &mut Steps) -> Option<Done> {
    let [x, y] = vars else { return None };
    let (dx, dy) = (domain_of(x)?, domain_of(y)?);
    if !dx.domain.integers() || !dy.domain.integers() {
        return None;
    }
    let f = expand(&sub(l.clone(), r.clone()));
    let (xs, ys) = (name(x), name(y));
    // ax + by + c0 = 0 with whole a, b, c0?
    let coef = |s: &Sym, v: &str| -> Option<i128> {
        let c = super::poly::coeffs(s, v)?;
        match c.len() {
            1 => Some(0),
            2 => match nice(&c[1]) {
                Sym::Num(q) if q.is_int() => Some(q.n),
                _ => None,
            },
            _ => None,
        }
    };
    let linear = (|| {
        let a = coef(&f, x)?;
        let b = coef(&f, y)?;
        let c0 = nice(&f.subs_var(x, &n(0)).subs_var(y, &n(0)));
        let Sym::Num(c0) = c0 else { return None };
        let c0 = c0.is_int().then_some(c0.n)?;
        // The x and y terms alone must account for everything else.
        let rebuilt = expand(&add(vec![
            mul(vec![n(a), var(x)]),
            mul(vec![n(b), var(y)]),
            n(c0),
        ]));
        (expand(&sub(f.clone(), rebuilt)).is_zero() && a != 0 && b != 0).then_some((a, b, -c0))
    })();
    let both_z = matches!(dx.domain, Domain::Num(crate::logic::NumSet::Integers))
        && matches!(dy.domain, Domain::Num(crate::logic::NumSet::Integers));
    if let (Some((a, b, c)), true) = (linear, both_z) {
        st.push(
            format!(
                "A linear equation in whole numbers: ${}{}={c}$",
                times(a, &xs),
                signed(b, &ys)
            ),
            "",
        );
        let (g, u, v) = gcd_ext(a, b);
        st.push(
            format!(
                "By Euclid's algorithm, $\\gcd({a},{b})={g}$, and ${a}\\cdot{}+{b}\\cdot{}={g}$",
                paren(u),
                paren(v)
            )
            .replace("+-", "-"),
            "",
        );
        if c % g != 0 {
            st.push(
                format!("${g}$ doesn't divide ${c}$, so there is no whole-number solution"),
                "",
            );
            return Some(Ok(("\\text{no solution in }\\mathbb{Z}".into(), None)));
        }
        let m = c / g;
        let (x0, y0) = (u * m, v * m);
        let multiply = if m == 1 {
            "One".to_owned()
        } else {
            format!("Multiply by ${m}$: one")
        };
        st.push(
            format!("{multiply} solution is ${xs}={x0},\\ {ys}={y0}$"),
            "",
        );
        let (sa, sb) = (b / g, a / g);
        let t = if x == "t" || y == "t" { "s" } else { "t" };
        let general_x = format!("{xs}={x0}{}", signed(sa, t));
        let general_y = format!("{ys}={y0}{}", signed(-sb, t));
        let result = format!("{general_x},\\ {general_y}\\quad\\left({t}\\in\\mathbb{{Z}}\\right)");
        st.push(
            format!("Adding ${sa}$ to ${xs}$ and taking ${sb}$ from ${ys}$ keeps the sum the same, so every solution is").replace("$-", "$-"),
            result.clone(),
        );
        return Some(Ok((result, None)));
    }
    // Search a box for solutions.
    let reach: i64 = 200;
    let lo = |d: &Domain| {
        d.least_integer()
            .map_or(-reach, |v| v.max(-reach as f64) as i64)
    };
    let hi = |d: &Domain| d.bounds().map_or(reach, |b| b.1.min(reach as f64) as i64);
    let mut found = Vec::new();
    for i in lo(&dx.domain)..=hi(&dx.domain) {
        for j in lo(&dy.domain)..=hi(&dy.domain) {
            let (fi, fj) = (i as f64, j as f64);
            if !dx.domain.contains(fi).unwrap_or(false) || !dy.domain.contains(fj).unwrap_or(false)
            {
                continue;
            }
            let w = f.subs_var(x, &n(i as i128)).at(y, fj);
            if w.is_finite() && w.abs() < 1e-9 {
                found.push((i, j));
            }
            if found.len() > 40 {
                break;
            }
        }
    }
    let box_note = format!(
        "Try every ${xs}$ and ${ys}$ from ${}$ to ${}$",
        lo(&dx.domain).min(lo(&dy.domain)),
        hi(&dx.domain).max(hi(&dy.domain))
    );
    if found.is_empty() {
        st.push(box_note, "");
        return Some(Ok(("\\text{no solution found}".into(), None)));
    }
    let shown: Vec<String> = found
        .iter()
        .take(40)
        .map(|(i, j)| format!("\\left({i},{j}\\right)"))
        .collect();
    let result = format!("\\left({xs},{ys}\\right)={}", shown.join(",\\ "));
    st.push(box_note, result.clone());
    Some(Ok((result, None)))
}

fn paren(v: i128) -> String {
    if v < 0 {
        format!("\\left({v}\\right)")
    } else {
        v.to_string()
    }
}

fn numeric(source: &str, others: &[String]) -> Result<String, String> {
    let mut rows: Vec<crate::RowInput> = others
        .iter()
        .enumerate()
        .map(|(i, l)| crate::RowInput {
            id: i as u64 + 1,
            latex: l.clone(),
            ..Default::default()
        })
        .collect();
    rows.push(crate::RowInput {
        id: rows.len() as u64 + 1,
        latex: source.to_owned(),
        ..Default::default()
    });
    let out = crate::analyze(&rows);
    let last = out.last().ok_or("Nothing to check")?;
    if let Some(e) = &last.error {
        return Err(e.clone());
    }
    last.verdict
        .clone()
        .ok_or_else(|| "This isn't a statement".into())
}

/// Solve on a statement: ∀ and ∃ proved where the algebra can, and
/// checked numerically everywhere else.
pub fn prove(prop: &Prop, source: &str, others: &[String], defs: &Defs) -> Report {
    let mut report = Report {
        op: "prove".into(),
        choices: vec![super::Choice {
            op: "prove".into(),
            label: "Prove or disprove".into(),
        }],
        ..Report::default()
    };
    let mut st = Steps::new();
    st.push("Decide whether this is true", source.to_owned());
    match symbolic_proof(prop, defs, &mut st) {
        Some(Ok(result)) => {
            report.result = Some(result);
            report.steps = st.list;
            return report;
        }
        Some(Err(e)) => st.push(
            format!("The algebra stops here ({e}), so check with numbers"),
            "",
        ),
        None => {}
    }
    match numeric(source, others) {
        Ok(verdict) => {
            st.push("Check it with numbers", verdict.clone());
            report.result = Some(verdict);
        }
        Err(e) => report.error = Some(e),
    }
    report.steps = st.list;
    report
}

/// The set a quantifier ranges over as typed, for the working.
fn set_latex(set: &SetExpr) -> String {
    match set {
        SetExpr::Num(n) => match n {
            crate::logic::NumSet::Naturals => "\\mathbb{N}".into(),
            crate::logic::NumSet::Integers => "\\mathbb{Z}".into(),
            crate::logic::NumSet::Rationals => "\\mathbb{Q}".into(),
            crate::logic::NumSet::Reals => "\\mathbb{R}".into(),
            other => other.symbol(),
        },
        _ => "S".into(),
    }
}

fn symbolic_proof(prop: &Prop, defs: &Defs, st: &mut Steps) -> Option<Result<String, String>> {
    let Prop::Quant(kind, v, set, body) = prop else {
        return None;
    };
    let Prop::Cmp(c) = &**body else { return None };
    if c.relations.len() != 1 {
        return None;
    }
    let l = to_sym(&c.operands[0], defs).ok()?;
    let r = to_sym(&c.operands[1], defs).ok()?;
    let rel = c.relations[0];
    let xs = name(v);
    let over_reals = matches!(set, SetExpr::Num(crate::logic::NumSet::Reals));
    let domain = Domain::from_set(set, &mut |e| numbers(e, defs)).ok()?;
    let f = sub(l.clone(), r.clone());
    if f.names().iter().any(|n| n != v && !is_constant_name(n)) {
        return None;
    }
    let in_set = format!("{xs}\\in {}", set_latex(set));
    match (kind, rel) {
        (Quant::All, Relation::Eq) => {
            let d = nice(&together(&expand(&f)));
            if d.is_zero() {
                st.push(
                    "Move everything to one side and simplify",
                    format!("{}=0", latex(&d)),
                );
                st.push(
                    format!("It is $0$ for every ${xs}$, so it holds for all ${in_set}$"),
                    "",
                );
                return Some(Ok("\\text{true}".into()));
            }
            None
        }
        (Quant::All, _) => {
            // ∀x: f ≥ 0 holds when f < 0 has no solution (anywhere, so in
            // any set).
            let negation = match rel {
                Relation::Lt => Relation::Ge,
                Relation::Le => Relation::Gt,
                Relation::Gt => Relation::Le,
                Relation::Ge => Relation::Lt,
                Relation::Eq => unreachable!(),
            };
            let mut inner = Steps::new();
            inner.push(
                "Look for a counterexample: solve the opposite",
                format!("{}{}{}", latex(&l), solve::rel_latex(negation), latex(&r)),
            );
            let out = solve::inequality(l.clone(), negation, r.clone(), v, &mut inner);
            match out {
                Ok((result, _)) if result.contains("no solution") => {
                    st.list.extend(inner.list);
                    st.push(
                        format!("Nothing breaks it, so it holds for all ${in_set}$"),
                        "",
                    );
                    Some(Ok("\\text{true}".into()))
                }
                Ok((result, _)) if over_reals => {
                    st.list.extend(inner.list);
                    // Name a counterexample.
                    let bad = [0.0, 1.0, -1.0, 0.5, -0.5, 2.0, -2.0, 10.0, -10.0]
                        .into_iter()
                        .find(|t| holds(negation, f.at(v, *t)));
                    let note = match bad {
                        Some(t) => {
                            format!("\\text{{false at }}{xs}={}", crate::format::value_latex(t))
                        }
                        None => "\\text{false}".into(),
                    };
                    st.push(
                        format!("It fails where {}", "$".to_owned() + &result + "$"),
                        note.clone(),
                    );
                    Some(Ok(note))
                }
                _ => None,
            }
        }
        (Quant::Exists | Quant::ExistsOne | Quant::NotExists, Relation::Eq) => {
            let mut inner = Steps::new();
            inner.push(
                format!("Solve for ${xs}$"),
                format!("{}={}", latex(&l), latex(&r)),
            );
            let roots = match solve::solve(l.clone(), r.clone(), v, &mut inner, 0) {
                Outcome::Roots(rs) => rs,
                Outcome::All => {
                    st.list.extend(inner.list);
                    let holds = *kind != Quant::NotExists;
                    return Some(Ok(if holds {
                        "\\text{true}".into()
                    } else {
                        "\\text{false}".into()
                    }));
                }
                Outcome::Fail => return None,
            };
            st.list.extend(inner.list);
            let mut kept: Vec<Sym> = Vec::new();
            for root in roots {
                let root = nice(&root);
                if !root.is_constant() {
                    return None;
                }
                let t = root.value();
                if domain.contains(t).unwrap_or(false)
                    && !kept.iter().any(|k| (k.value() - t).abs() < 1e-9)
                    && (f.at(v, t)).abs() < 1e-7
                {
                    kept.push(root);
                }
            }
            kept.sort_by(|a, b| a.value().total_cmp(&b.value()));
            let shown: Vec<String> = kept.iter().map(|k| format!("{xs}={}", latex(k))).collect();
            if !over_reals {
                st.push(format!("Keep those with ${in_set}$"), shown.join(",\\ "));
            }
            let result = match (kind, kept.len()) {
                (Quant::Exists, 0) | (Quant::ExistsOne, 0) => {
                    "\\text{false: no solution}".to_owned()
                }
                (Quant::Exists, _) => format!("\\text{{true: }}{}", shown.join(",\\ ")),
                (Quant::ExistsOne, 1) => format!("\\text{{true: }}{}", shown[0]),
                (Quant::ExistsOne, _) => {
                    format!("\\text{{false: }}{}", shown.join("\\text{ and }"))
                }
                (Quant::NotExists, 0) => "\\text{true}".to_owned(),
                (Quant::NotExists, _) => format!("\\text{{false: }}{}", shown[0]),
                _ => unreachable!(),
            };
            Some(Ok(result))
        }
        _ => None,
    }
}

fn holds(rel: Relation, v: f64) -> bool {
    match rel {
        Relation::Lt => v < 0.0,
        Relation::Le => v <= 0.0,
        Relation::Gt => v > 0.0,
        Relation::Ge => v >= 0.0,
        Relation::Eq => v == 0.0,
    }
}
