//! n-fold integrals written with dots, such as
//! `\int_1^2\int_2^3...\int_n^{n+1} f(x_1x_2...x_n)\,dx_1dx_2...dx_n`.
//!
//! When the integrand depends only on the product P = x₁x₂⋯xₙ and is a sum
//! of terms c·Pᵃ and c·Pᵃ·ln P, the integral splits into one-variable
//! integrals: ∫⋯∫ Pᵃ = ∏ Hₖ and ∫⋯∫ Pᵃ ln P = ∏ Hₖ · Σ Gₖ/Hₖ, with
//! Hₖ = ∫ xᵃ and Gₖ = ∫ xᵃ ln x over the k-th interval. The product and
//! the sum are then found in closed form where they can be (factorials, a
//! sum of powers, logarithms that telescope), and the answer is checked
//! against the integral worked out numerically for small n.

use super::convert::{to_sym, Defs};
use super::latex::{latex, name};
use super::sym::*;
use super::{integrate, nice, poly, Steps};
use crate::dots::{self, Chain};
use crate::lexer::{tokenize, Tok};

type Done = Result<(String, Option<String>), String>;

const NOT_SEPARABLE: &str = "Solve works out n-fold integrals written with ... when the integrand depends only on the product x_1x_2...x_n, such as \\ln(x_1x_2...x_n)/(x_1x_2...x_n)^2";

/// A term c·Pᵃ·(ln P)ᵐ of the integrand.
#[derive(Clone, Debug)]
struct Term {
    c: Sym,
    a: Sym,
    m: u32,
}

/// The parts of an n-fold integral as written.
struct Parts {
    /// `I_n` in `I_n = ...`, if written.
    lhs: Option<String>,
    /// The integrand with P for the product.
    body: Sym,
    /// The k-th interval, in the index `k`.
    lo: Sym,
    hi: Sym,
    /// How many integrals: n.
    count: Sym,
    /// The letter n stands for, to try small values of it.
    letter: Option<char>,
    var: String,
    /// The integral as written, without `I_n =`.
    rhs: String,
}

/// Placeholders for factorials and for products and sums left as they
/// are, put back into the LaTeX at the end.
#[derive(Default)]
struct Opaque {
    list: Vec<(String, String)>,
}

impl Opaque {
    fn add(&mut self, latex: String) -> Sym {
        if let Some((name, _)) = self.list.iter().find(|(_, l)| *l == latex) {
            return var(name);
        }
        let name = format!("QQo{}", (b'a' + self.list.len() as u8) as char);
        self.list.push((name.clone(), latex));
        var(&name)
    }

    fn factorial(&mut self, of: &Sym) -> Sym {
        if let Some(q) = of.as_q().filter(|q| q.is_int() && q.n >= 0 && q.n <= 20) {
            return n((1..=q.n).product::<i128>().max(1));
        }
        let inner = match of {
            Sym::Var(_) => latex(of),
            _ => format!("\\left({}\\right)", latex(of)),
        };
        self.add(format!("{inner}!"))
    }

    fn put_back(&self, mut tex: String) -> String {
        // Longest names first, so none is a prefix of another.
        for (name, l) in self.list.iter().rev() {
            let wrapped = format!("\\left({l}\\right)");
            for (from, to) in [
                (format!("\\ln \\{name} "), format!("\\ln{wrapped}")),
                (format!("\\ln\\{name} "), format!("\\ln{wrapped}")),
                (format!("\\{name} ^"), format!("{wrapped}^")),
                (format!("\\{name} "), l.clone()),
                (format!("\\{name}"), l.clone()),
            ] {
                tex = tex.replace(&from, &to);
            }
        }
        tex
    }
}

/// Solves a row with `...` that only Solve can work with.
pub(super) fn solve(source: &str, others: &[String], defs: &Defs, st: &mut Steps) -> Done {
    let parts = read(source, defs)?;
    let terms = decompose(&parts.body, "QQP").ok_or(NOT_SEPARABLE)?;
    let terms = combine(terms);
    let k = "k";
    let x = parts.var.as_str();
    let xs = format!(
        "{}_{{1}}{}_{{2}}\\cdots {}_{{{}}}",
        name(x),
        name(x),
        name(x),
        latex(&parts.count)
    );
    let count = parts.count.clone();
    let shown_body = latex(&parts.body)
        .replace("\\QQP ", "P")
        .replace("\\QQP", "P");
    st.push(
        format!("The integrand depends only on the product $P={xs}$"),
        format!("f\\left(P\\right)={shown_body}"),
    );
    if terms.iter().any(|t| t.m == 1) {
        st.push(
            "The logarithm of a product is the sum of the logarithms",
            format!(
                "\\ln P=\\ln {v}_{{1}}+\\ln {v}_{{2}}+\\cdots+\\ln {v}_{{{c}}}",
                v = name(x),
                c = latex(&count),
            ),
        );
    }
    let interval = format!("\\left[{},{}\\right]", latex(&parts.lo), latex(&parts.hi));
    st.push(
        format!("Each variable has its own interval, the $k$th being ${interval}$, so the integral of a product of powers splits into one-variable integrals"),
        "",
    );

    let mut opaque = Opaque::default();
    let mut total = Vec::new();
    let groups = terms.len();
    for (gi, t) in terms.iter().enumerate() {
        let tag = if groups > 1 {
            format!("^{{({})}}", gi + 1)
        } else {
            String::new()
        };
        let h_name = format!("H{tag}_{{k}}");
        let g_name = format!("G{tag}_{{k}}");
        let power = pow(var(x), t.a.clone());
        let h = definite(&power, x, &parts.lo, &parts.hi).ok_or(NOT_SEPARABLE)?;
        st.push(
            format!("The one-variable integral for $P^{{{}}}$", latex(&t.a)),
            format!(
                "{h_name}=\\int_{{{}}}^{{{}}}{}\\ d{}={}",
                latex(&parts.lo),
                latex(&parts.hi),
                latex(&power),
                name(x),
                latex(&h)
            ),
        );
        let prod = product(&h, k, &count, &mut opaque);
        st.push(
            "Multiply them for every variable",
            format!(
                "\\prod_{{k=1}}^{{{}}}{h_name}={}",
                latex(&count),
                opaque.put_back(latex(&prod))
            ),
        );
        let mut value = mul2(t.c.clone(), prod);
        if t.m == 1 {
            let with_ln = mul2(power.clone(), func(Func::Ln, var(x)));
            let g = definite(&with_ln, x, &parts.lo, &parts.hi).ok_or(NOT_SEPARABLE)?;
            st.push(
                "With the logarithm, only one variable's integral changes in each term of the sum",
                format!(
                    "\\int\\cdots\\int P^{{{}}}\\ln P=\\left(\\prod_{{k=1}}^{{{}}}{h_name}\\right)\\sum_{{k=1}}^{{{}}}\\frac{{{g_name}}}{{{h_name}}}",
                    latex(&t.a),
                    latex(&count),
                    latex(&count),
                ),
            );
            st.push(
                "where",
                format!(
                    "{g_name}=\\int_{{{}}}^{{{}}}{}\\ d{}={}",
                    latex(&parts.lo),
                    latex(&parts.hi),
                    latex(&with_ln),
                    name(x),
                    latex(&g)
                ),
            );
            let ratio = ratio(&g, &h);
            let s = sum(&ratio, k, &count, &mut opaque);
            st.push(
                "Add up the ratios; the logarithms telescope",
                format!(
                    "\\sum_{{k=1}}^{{{}}}\\frac{{{g_name}}}{{{h_name}}}=\\sum_{{k=1}}^{{{}}}\\left({}\\right)={}",
                    latex(&count),
                    latex(&count),
                    latex(&ratio),
                    opaque.put_back(latex(&s))
                ),
            );
            value = mul2(value, s);
        }
        total.push(value);
    }
    let answer = nice(&add(total));
    let answer_tex = opaque.put_back(latex(&answer));
    let shown = match &parts.lhs {
        Some(l) => format!("{l}={answer_tex}"),
        None => answer_tex.clone(),
    };
    st.push("So", shown.clone());
    let checked = check(&parts, &answer_tex, others)?;
    st.push(
        format!(
            "Checked against the integral worked out numerically for ${}$",
            checked
        ),
        "",
    );
    Ok((answer_tex.clone(), Some(shown)))
}

/// Reads `I_n = \int...\int f dx_1...dx_n`.
fn read(source: &str, defs: &Defs) -> Result<Parts, String> {
    let toks = tokenize(source)?;
    let (lhs, from) = match toks.iter().position(|t| *t == Tok::Eq) {
        Some(i) => {
            let (l, _) = source.split_once('=').ok_or(NOT_SEPARABLE)?;
            (Some(l.trim().to_owned()), i + 1)
        }
        None => (None, 0),
    };
    let rhs = match source.split_once('=') {
        Some((_, r)) if lhs.is_some() => r.trim().to_owned(),
        _ => source.trim().to_owned(),
    };
    let all = dots::dots_outside_lists(&toks);
    let first = *all.first().ok_or(NOT_SEPARABLE)?;
    let ints = dots::chain(&toks, first)?;
    if ints.start != from || ints.unit.first() != Some(&Tok::Int) {
        return Err(NOT_SEPARABLE.into());
    }
    // The bounds in the index k.
    let unit = in_index(&ints, "k");
    let (lo, next) = script(&unit, 2).ok_or(NOT_SEPARABLE)?;
    if unit.get(1) != Some(&Tok::Underscore) || unit.get(next) != Some(&Tok::Caret) {
        return Err("Each integral needs bounds, like \\int_1^2".into());
    }
    let (hi, end) = script(&unit, next + 1).ok_or(NOT_SEPARABLE)?;
    if end != unit.len() {
        return Err(NOT_SEPARABLE.into());
    }
    // The differentials, last; they say how many integrals there are.
    let last = *all.last().ok_or(NOT_SEPARABLE)?;
    let ds = dots::chain(&toks, last)?;
    let v = match ds.unit.as_slice() {
        [Tok::Letter(d), Tok::Letter(v), Tok::Underscore, ..] if d == "d" => v.clone(),
        _ => {
            return Err(
                "End an n-fold integral with its differentials, like dx_1dx_2...dx_n".into(),
            )
        }
    };
    if ds.end != toks.len() || ds.holes.len() != 1 || !ds.holes[0].name {
        return Err("Give each integral its own differential, like dx_1dx_2...dx_n".into());
    }
    let hole = &ds.holes[0];
    let count_toks = hole.count_tokens();
    let sym = |t: Vec<Tok>| -> Result<Sym, String> {
        let e = crate::parser::parse_tokens(t)?;
        to_sym(&e, defs)
    };
    let count = sym(count_toks.clone())?;
    for h in &ints.holes {
        if !expand(&sub(sym(h.count_tokens())?, count.clone())).is_zero() {
            return Err("There should be as many integrals as differentials".into());
        }
    }
    let letters: Vec<String> = hole
        .last
        .iter()
        .filter_map(|t| match t {
            Tok::Letter(l) => Some(l.clone()),
            _ => None,
        })
        .collect();
    let letter = match letters.as_slice() {
        [l] if l.chars().count() == 1 => l.chars().next(),
        _ => None,
    };
    // The integrand, with P for each product of the variables.
    let p = Tok::Letter("QQP".into());
    let mut body = Vec::new();
    let mut i = ints.end;
    while i < ds.start {
        if toks[i] == Tok::Letter(v.clone()) {
            // The start of a chain x_1x_2...x_n (or x_1^2x_2^2...x_n^2).
            let chain = all
                .iter()
                .filter(|&&d| d > i && d < ds.start)
                .find_map(|&d| dots::chain(&toks, d).ok().filter(|c| c.start == i));
            let Some(c) = chain else {
                return Err(NOT_SEPARABLE.into());
            };
            let ok = c.sep.is_none()
                && c.holes.len() == 1
                && c.holes[0].name
                && sym(c.holes[0].count_tokens())
                    .is_ok_and(|m| expand(&sub(m, count.clone())).is_zero())
                && c.unit.first() == Some(&Tok::Letter(v.clone()));
            if !ok {
                return Err(NOT_SEPARABLE.into());
            }
            // What follows the subscript, such as ^2, is a power of P.
            let after = match script(&c.unit, 2) {
                Some((_, j)) => c.unit[j..].to_vec(),
                None => return Err(NOT_SEPARABLE.into()),
            };
            if after.iter().any(|t| *t == Tok::Letter(v.clone())) {
                return Err(NOT_SEPARABLE.into());
            }
            body.push(Tok::LParen);
            body.push(p.clone());
            body.extend(after);
            body.push(Tok::RParen);
            i = c.end;
            continue;
        }
        if toks[i] == Tok::Dots {
            return Err(NOT_SEPARABLE.into());
        }
        body.push(toks[i].clone());
        i += 1;
    }
    let body = sym(body)?;
    let lo = sym(lo)?;
    let hi = sym(hi)?;
    if lo.has(&v) || hi.has(&v) || body.has(&v) {
        return Err(NOT_SEPARABLE.into());
    }
    Ok(Parts {
        lhs,
        body,
        lo,
        hi,
        count,
        letter,
        var: v,
        rhs,
    })
}

/// The chain's term with the changing places in the index.
fn in_index(c: &Chain, index: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    for (i, t) in c.unit.iter().enumerate() {
        match c.holes.iter().find(|h| h.at == i) {
            Some(h) => out.extend(h.value_tokens(index)),
            None => out.push(t.clone()),
        }
    }
    out
}

/// A sub- or superscript starting at `toks[at]`: its tokens and where it
/// ends.
fn script(toks: &[Tok], at: usize) -> Option<(Vec<Tok>, usize)> {
    match toks.get(at)? {
        Tok::LBrace | Tok::LParen => {
            let mut depth = 0;
            for (i, t) in toks.iter().enumerate().skip(at) {
                match t {
                    Tok::LBrace | Tok::LParen => depth += 1,
                    Tok::RBrace | Tok::RParen => {
                        depth -= 1;
                        if depth == 0 {
                            let inner = if toks[at] == Tok::LBrace {
                                toks[at + 1..i].to_vec()
                            } else {
                                toks[at..=i].to_vec()
                            };
                            return Some((inner, i + 1));
                        }
                    }
                    _ => {}
                }
            }
            None
        }
        t => Some((vec![t.clone()], at + 1)),
    }
}

// ---------------------------------------------------------------------------
// The integrand as a sum of c·Pᵃ·(ln P)ᵐ

fn decompose(s: &Sym, p: &str) -> Option<Vec<Term>> {
    if !s.has(p) {
        return Some(vec![Term {
            c: s.clone(),
            a: n(0),
            m: 0,
        }]);
    }
    match s {
        Sym::Var(v) if v == p => Some(vec![Term {
            c: n(1),
            a: n(1),
            m: 0,
        }]),
        Sym::Add(xs) => {
            let mut out = Vec::new();
            for x in xs {
                out.extend(decompose(x, p)?);
            }
            Some(out)
        }
        Sym::Mul(xs) => {
            let mut acc = vec![Term {
                c: n(1),
                a: n(0),
                m: 0,
            }];
            for x in xs {
                acc = times(&acc, &decompose(x, p)?)?;
            }
            Some(acc)
        }
        Sym::Pow(b, e) if !e.has(p) => {
            let bt = decompose(b, p)?;
            match bt.as_slice() {
                [t] if t.m == 0 => Some(vec![Term {
                    c: pow(t.c.clone(), (**e).clone()),
                    a: mul2(t.a.clone(), (**e).clone()),
                    m: 0,
                }]),
                _ => {
                    let k = e.as_q().filter(|q| q.is_int() && q.n > 0 && q.n <= 4)?.n;
                    let mut acc = bt.clone();
                    for _ in 1..k {
                        acc = times(&acc, &bt)?;
                    }
                    Some(acc)
                }
            }
        }
        Sym::Fn(Func::Ln, arg) => match decompose(arg, p)?.as_slice() {
            [t] if t.m == 0 => {
                let mut out = vec![Term {
                    c: t.a.clone(),
                    a: n(0),
                    m: 1,
                }];
                if !t.c.is_one() {
                    out.push(Term {
                        c: func(Func::Ln, t.c.clone()),
                        a: n(0),
                        m: 0,
                    });
                }
                Some(out)
            }
            _ => None,
        },
        _ => None,
    }
}

fn times(a: &[Term], b: &[Term]) -> Option<Vec<Term>> {
    let mut out = Vec::new();
    for x in a {
        for y in b {
            let m = x.m + y.m;
            if m > 1 {
                return None;
            }
            out.push(Term {
                c: mul2(x.c.clone(), y.c.clone()),
                a: add2(x.a.clone(), y.a.clone()),
                m,
            });
        }
    }
    Some(out)
}

/// Like terms together: one per power and logarithm.
fn combine(terms: Vec<Term>) -> Vec<Term> {
    let mut out: Vec<Term> = Vec::new();
    for t in terms {
        match out.iter_mut().find(|u| u.a == t.a && u.m == t.m) {
            Some(u) => u.c = add2(u.c.clone(), t.c),
            None => out.push(t),
        }
    }
    out.retain(|t| !t.c.is_zero());
    out
}

// ---------------------------------------------------------------------------
// The one-variable integrals, and their product and sum over k

fn definite(f: &Sym, x: &str, lo: &Sym, hi: &Sym) -> Option<Sym> {
    let big = integrate::antiderivative(f, x, &mut Steps::quiet())?;
    Some(nice(&sub(big.subs_var(x, hi), big.subs_var(x, lo))))
}

/// G/H with each term of G divided separately, so factors cancel.
fn ratio(g: &Sym, h: &Sym) -> Sym {
    let (top, bottom) = num_den(&together(h));
    let inverse = div(bottom, top);
    let terms: Vec<Sym> = expand(g)
        .terms()
        .into_iter()
        .map(|t| expand(&mul2(t, inverse.clone())))
        .collect();
    expand(&add(terms))
}

/// ∏ₖ₌₁ᴺ h(k), with factorials when h is a ratio of shifted k's.
fn product(h: &Sym, k: &str, count: &Sym, opaque: &mut Opaque) -> Sym {
    closed_product(h, k, count, opaque).unwrap_or_else(|| {
        opaque.add(format!(
            "\\prod_{{{k}=1}}^{{{}}}\\left({}\\right)",
            latex(count),
            latex(h)
        ))
    })
}

fn closed_product(h: &Sym, k: &str, count: &Sym, opaque: &mut Opaque) -> Option<Sym> {
    let (top, bottom) = num_den(&together(h));
    let side = |s: &Sym, opaque: &mut Opaque| -> Option<Sym> {
        let p = poly::rational(&poly::coeffs(&expand(s), k)?)?;
        let f = poly::factor(&p)?;
        let mut parts = vec![pow(Sym::Num(f.unit), count.clone())];
        for (c, m) in &f.factors {
            // c[0] + c[1] k = c[1] (k + c[0]/c[1]).
            let [c0, c1] = c.as_slice() else { return None };
            let shift = Q::new(*c0, *c1)?;
            if !shift.is_int() || shift.n < 0 {
                return None;
            }
            let fac = opaque.factorial(&add2(count.clone(), n(shift.n)));
            let below = opaque.factorial(&n(shift.n));
            let one = mul(vec![pow(n(*c1), count.clone()), fac, pow(below, n(-1))]);
            parts.push(powi(one, *m as i128));
        }
        Some(mul(parts))
    };
    let t = side(&top, opaque)?;
    let b = side(&bottom, opaque)?;
    Some(div(t, b))
}

/// Σₖ₌₁ᴺ r(k), in closed form when r is a polynomial plus polynomials
/// times logarithms of k + c whose sum telescopes.
fn sum(r: &Sym, k: &str, count: &Sym, opaque: &mut Opaque) -> Sym {
    closed_sum(r, k, count, opaque).unwrap_or_else(|| {
        opaque.add(format!(
            "\\sum_{{{k}=1}}^{{{}}}\\left({}\\right)",
            latex(count),
            latex(r)
        ))
    })
}

fn closed_sum(r: &Sym, k: &str, count: &Sym, opaque: &mut Opaque) -> Option<Sym> {
    let mut plain = Vec::new();
    // (c, p(k)) for p(k) ln(k + c).
    let mut logs: Vec<(i128, Sym)> = Vec::new();
    for t in expand(r).terms() {
        let factors = t.factors();
        let lns: Vec<usize> = factors
            .iter()
            .enumerate()
            .filter(|(_, f)| matches!(f, Sym::Fn(Func::Ln, a) if a.has(k)))
            .map(|(i, _)| i)
            .collect();
        match lns.as_slice() {
            [] => plain.push(t.clone()),
            [i] => {
                let Sym::Fn(_, arg) = &factors[*i] else {
                    unreachable!()
                };
                let c = poly::coeffs(arg, k)?;
                let shift = match c.as_slice() {
                    [c0, c1] if c1.is_one() => c0.as_q().filter(|q| q.is_int())?.n,
                    _ => return None,
                };
                let rest: Vec<Sym> = factors
                    .iter()
                    .enumerate()
                    .filter(|(j, _)| j != i)
                    .map(|(_, f)| f.clone())
                    .collect();
                let rest = mul(rest);
                poly::coeffs(&rest, k)?;
                match logs.iter_mut().find(|(s, _)| *s == shift) {
                    Some(e) => e.1 = add2(e.1.clone(), rest),
                    None => logs.push((shift, rest)),
                }
            }
            _ => return None,
        }
    }
    let mut out = vec![power_sum(&add(plain), k, count)?];
    if !logs.is_empty() {
        // Σ_{k=1}^{N} p_c(k) ln(k+c) = Σ_{j=1+c}^{N+c} p_c(j-c) ln j: over the
        // range they share the coefficients add up, and the ends are a few
        // terms each.
        let low = logs.iter().map(|(c, _)| 1 + c).max()?;
        let cmin = logs.iter().map(|(c, _)| *c).min()?;
        if low < 1 {
            return None;
        }
        let j = "QQj";
        let mut shared = Vec::new();
        for (c, p) in &logs {
            let shifted = expand(&p.subs_var(k, &sub(var(j), n(*c))));
            shared.push(shifted.clone());
            // j from 1 + c up to low - 1: numbers.
            for jj in 1 + c..low {
                let coef = shifted.subs_var(j, &n(jj));
                out.push(mul2(coef, func(Func::Ln, n(jj))));
            }
            // j from N + cmin + 1 up to N + c.
            for t in 1..=(c - cmin) {
                let at = add2(count.clone(), n(cmin + t));
                let coef = expand(&shifted.subs_var(j, &at));
                out.push(mul2(coef, func(Func::Ln, at)));
            }
        }
        let shared = expand(&add(shared));
        if !shared.is_zero() {
            if shared.has(j) {
                return None;
            }
            // shared · ln((N + cmin)! / (low - 1)!).
            let top = opaque.factorial(&add2(count.clone(), n(cmin)));
            let bottom = opaque.factorial(&n(low - 1));
            out.push(mul2(shared.clone(), func(Func::Ln, top)));
            out.push(neg(mul2(shared, func(Func::Ln, bottom))));
        }
    }
    Some(add(out))
}

/// Σₖ₌₁ᴺ of a polynomial in k, up to cubes.
fn power_sum(p: &Sym, k: &str, count: &Sym) -> Option<Sym> {
    let c = poly::coeffs(&expand(p), k)?;
    let big = count.clone();
    let s1 = || div(mul2(big.clone(), add2(big.clone(), n(1))), n(2));
    let sums = [
        big.clone(),
        s1(),
        div(
            mul(vec![
                big.clone(),
                add2(big.clone(), n(1)),
                add2(mul2(n(2), big.clone()), n(1)),
            ]),
            n(6),
        ),
        powi(s1(), 2),
    ];
    let mut out = Vec::new();
    for (i, ci) in c.iter().enumerate() {
        if ci.is_zero() {
            continue;
        }
        out.push(mul2(ci.clone(), sums.get(i)?.clone()));
    }
    Some(expand(&add(out)))
}

// ---------------------------------------------------------------------------
// The check

/// Compares the answer with the integral worked out numerically for the
/// first few counts; says which ones it checked.
fn check(parts: &Parts, answer: &str, others: &[String]) -> Result<String, String> {
    let Some(letter) = parts.letter else {
        return Err("Solve couldn't check this answer, so it isn't shown".into());
    };
    let value = |latex: &str| -> Option<f64> {
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
            id: 0,
            latex: latex.to_owned(),
            ..Default::default()
        });
        match crate::analyze(&rows).last()?.value {
            Some(crate::Value::Number(v)) if v.is_finite() => Some(v),
            _ => None,
        }
    };
    let mut done = Vec::new();
    for v in 1..=12 {
        let at = v.to_string();
        let count = parts.count.subs_var(&letter.to_string(), &n(v)).value();
        if !((1.0..=3.0).contains(&count) && (count - count.round()).abs() < 1e-9) {
            continue;
        }
        let written = dots::set_letter(&parts.rhs, letter, &at);
        let (Some(want), Some(got)) = (
            value(&written),
            value(&dots::set_letter(answer, letter, &at)),
        ) else {
            return Err("Solve couldn't check this answer, so it isn't shown".into());
        };
        if (want - got).abs() > 1e-6 * want.abs().max(1.0) {
            return Err(format!(
                "Solve's answer didn't match the integral worked out numerically for {letter} = {v}, so it isn't shown"
            ));
        }
        done.push(format!("{}={v}", letter));
        if done.len() == 3 {
            break;
        }
    }
    if done.is_empty() {
        return Err("Solve couldn't check this answer, so it isn't shown".into());
    }
    Ok(done.join(",\\ "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(latex: &str) -> super::super::Report {
        super::super::run(latex, &[], None)
    }

    #[test]
    fn n_fold_integral_of_a_product() {
        let r = run("I_n=\\int _1^2\\int _2^3...\\int _n^{n+1}\\frac{\\ln \\left(x_1x_2...x_n\\right)}{\\left(x_1x_2...x_n\\right)^2}dx_1dx_2...dx_n");
        assert_eq!(r.error, None);
        assert_eq!(
            r.result.as_deref(),
            Some(
                "\\frac{-n\\ln\\left(n+1\\right)+n+2\\ln\\left(n!\\right)}{n!\\left(n+1\\right)!}"
            )
        );
        assert!(r.answer.as_deref().unwrap().starts_with("I_n="));
        assert!(r.steps.iter().any(|s| s.note.starts_with("Checked")));
        // As the editor writes it, with braces and \ldots.
        let r = run("\\int_{1}^{2}\\int_{2}^{3}\\ldots\\int_{n}^{n+1}\\frac{\\ln\\left(x_{1}x_{2}\\ldots x_{n}\\right)}{\\left(x_{1}x_{2}\\ldots x_{n}\\right)^{2}}dx_{1}dx_{2}\\ldots dx_{n}");
        assert_eq!(r.error, None, "{:?}", r.steps);
    }

    #[test]
    fn other_n_fold_integrals() {
        let r = run("\\int_0^1\\int_0^1...\\int_0^1x_1x_2...x_ndx_1dx_2...dx_n");
        assert_eq!(r.error, None);
        assert_eq!(r.result.as_deref(), Some("2^{-n}"));
        let r = run("\\int_1^2\\int_1^2...\\int_1^2\\left(x_1x_2...x_n\\right)^3+\\ln\\left(x_1x_2...x_n\\right)dx_1dx_2...dx_n");
        assert_eq!(r.error, None, "{:?}", r.steps);
        // Not a function of the product.
        let r = run("\\int_0^1\\int_0^1...\\int_0^1\\left(x_1+x_2+...+x_n\\right)dx_1dx_2...dx_n");
        assert_eq!(r.error.as_deref(), Some(NOT_SEPARABLE));
    }

    #[test]
    fn wrong_answers_are_caught() {
        let parts = read(
            "\\int_0^1\\int_0^1...\\int_0^1x_1x_2...x_ndx_1dx_2...dx_n",
            &Defs::default(),
        )
        .unwrap();
        assert!(check(&parts, "\\frac{1}{2^{n}}", &[]).is_ok());
        assert!(check(&parts, "\\frac{1}{n+1}", &[]).is_err());
    }
}
