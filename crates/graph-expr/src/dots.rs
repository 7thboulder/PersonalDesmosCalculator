//! `...` for repetition outside lists, as written on paper:
//! `1+2+...+n`, `x_1x_2...x_n` or `\int_1^2\int_2^3...\int_n^{n+1}`.
//!
//! The terms before the dots set a pattern: the last two written agree
//! except for numbers that step by a fixed amount (`a_1+` then `a_2+`), and
//! the term after the dots fills those places with its own values (`a_n`),
//! which give the count. A chain with a known count is written out in
//! full. One with an unknown count becomes a sum or a product with an index
//! when it can (`1+2+...+n` is Σ k for k = 1 to n); anything else, such as
//! an n-fold integral, is left to Solve and reads as `SYMBOLIC`.

use crate::lexer::Tok;

/// The error a line with a chain of unknown length gives: valid, but only
/// Solve works with it.
pub const SYMBOLIC: &str = "\u{7}symbolic dots";

/// Most terms a chain is written out to.
const MOST: usize = 200;

/// A chain of terms around one `...`.
#[derive(Clone, Debug, PartialEq)]
pub struct Chain {
    /// Where the chain starts and ends in the tokens (end exclusive).
    pub start: usize,
    pub end: usize,
    /// One term, with numbers in the places that change.
    pub unit: Vec<Tok>,
    /// What comes between terms: `+`, `\cdot`, `,` or nothing.
    pub sep: Option<Tok>,
    pub holes: Vec<Hole>,
}

/// A place in the term that changes from one term to the next.
#[derive(Clone, Debug, PartialEq)]
pub struct Hole {
    pub at: usize,
    /// Its value in the first term, and how much it grows each term.
    pub first: f64,
    pub step: f64,
    /// What the last term has there.
    pub last: Vec<Tok>,
    /// A subscript of a name (`x_1`), not a number in the maths.
    pub name: bool,
}

impl Hole {
    /// The number of terms, when the last value is a number.
    fn count(&self) -> Option<usize> {
        let last = number(&self.last)?;
        let n = (last - self.first) / self.step + 1.0;
        (n >= 1.0 && (n - n.round()).abs() < 1e-9).then_some(n.round() as usize)
    }

    /// The number of terms as tokens: `n` for `a_1 ... a_n`, else
    /// `(last - first) / step + 1`.
    pub fn count_tokens(&self) -> Vec<Tok> {
        let mut last = vec![Tok::LParen];
        last.extend(self.last.iter().cloned());
        last.push(Tok::RParen);
        if self.first == 1.0 && self.step == 1.0 {
            return last;
        }
        let mut t = vec![Tok::LParen];
        t.extend(last);
        t.push(Tok::Minus);
        t.push(Tok::Num(self.first));
        t.push(Tok::RParen);
        if self.step != 1.0 {
            t.push(Tok::Div);
            t.push(Tok::Num(self.step));
        }
        t.extend([Tok::Plus, Tok::Num(1.0)]);
        t
    }

    /// The value in term `k` (from 0), as tokens in the index `index`.
    pub fn value_tokens(&self, index: &str) -> Vec<Tok> {
        let k = Tok::Letter(index.to_owned());
        if self.first == 1.0 && self.step == 1.0 {
            return vec![k];
        }
        // first + step (k - 1)
        vec![
            Tok::LParen,
            Tok::Num(self.first),
            Tok::Plus,
            Tok::Num(self.step),
            Tok::LParen,
            k,
            Tok::Minus,
            Tok::Num(1.0),
            Tok::RParen,
            Tok::RParen,
        ]
    }
}

fn number(toks: &[Tok]) -> Option<f64> {
    match toks {
        [Tok::Num(v)] => Some(*v),
        [Tok::Minus, Tok::Num(v)] => Some(-v),
        [Tok::LBrace, inner @ .., Tok::RBrace] => number(inner),
        _ => None,
    }
}

fn is_sep(t: &Tok) -> bool {
    matches!(t, Tok::Plus | Tok::Mul | Tok::Comma)
}

/// Whether brackets of every kind open and close within `toks`.
fn balanced(toks: &[Tok]) -> bool {
    let mut depth = [0i32; 4];
    for t in toks {
        let (i, d) = match t {
            Tok::LBrace => (0, 1),
            Tok::RBrace => (0, -1),
            Tok::LParen => (1, 1),
            Tok::RParen => (1, -1),
            Tok::LBracket => (2, 1),
            Tok::RBracket => (2, -1),
            Tok::AbsOpen => (3, 1),
            Tok::AbsClose => (3, -1),
            _ => continue,
        };
        depth[i] += d;
        if depth[i] < 0 {
            return false;
        }
    }
    depth == [0; 4]
}

/// Where the brace group starting at `toks[at]` (a `{`) closes.
fn group_end(toks: &[Tok], at: usize) -> Option<usize> {
    let mut depth = 0;
    for (i, t) in toks.iter().enumerate().skip(at) {
        match t {
            Tok::LBrace | Tok::LParen => depth += 1,
            Tok::RBrace | Tok::RParen => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The places where term `a` and the next term `b` differ: numbers that
/// step. None when they differ in anything else.
fn diff(a: &[Tok], b: &[Tok]) -> Option<Vec<(usize, f64, f64)>> {
    let mut holes = Vec::new();
    for (i, (x, y)) in a.iter().zip(b).enumerate() {
        if x == y {
            continue;
        }
        match (x, y) {
            (Tok::Num(p), Tok::Num(q)) if q != p => holes.push((i, *p, q - p)),
            _ => return None,
        }
    }
    Some(holes)
}

/// The chain around the `...` at `dots`, or why it can't be read.
pub fn chain(toks: &[Tok], dots: usize) -> Result<Chain, String> {
    let before = &toks[..dots];
    let n = before.len();
    let cannot =
        || "Write at least two terms before the ..., such as 1+2+...+n or x_1x_2...x_n".to_owned();
    // The shortest term that the last two terms before the dots repeat.
    let (p, steps) = (1..=n / 2)
        .find_map(|p| {
            let (a, b) = (&before[n - 2 * p..n - p], &before[n - p..]);
            if !balanced(b) {
                return None;
            }
            let holes = diff(a, b)?;
            (!holes.is_empty()).then_some((p, holes))
        })
        .or_else(|| {
            // The same term over and over, as in \int_0^1\int_0^1...\int_0^1:
            // its length comes from elsewhere.
            (1..=n / 2)
                .find(|&p| {
                    let (a, b) = (&before[n - 2 * p..n - p], &before[n - p..]);
                    balanced(b) && a == b
                })
                .map(|p| (p, Vec::new()))
        })
        .ok_or_else(cannot)?;
    let unit_with_sep: Vec<Tok> = before[n - p..].to_vec();
    // Earlier terms in the same pattern belong to the chain too.
    let mut start = n - 2 * p;
    let mut firsts: Vec<f64> = steps.iter().map(|&(_, a, _)| a).collect();
    while start >= p {
        let earlier = &before[start - p..start];
        let ok = !steps.is_empty()
            && steps
                .iter()
                .zip(&firsts)
                .all(|(&(i, _, step), &v)| earlier[i] == Tok::Num(v - step))
            && diff(earlier, &before[start..start + p]).is_some_and(|h| h.len() == steps.len());
        let same = steps.is_empty() && earlier == &before[start..start + p];
        if !ok && !same {
            break;
        }
        start -= p;
        for (v, &(_, _, step)) in firsts.iter_mut().zip(&steps) {
            *v -= step;
        }
    }
    // A separator between terms ends each term before the dots and starts
    // the last one after them: + ... + a_n.
    let (sep, unit) = match unit_with_sep.last() {
        Some(t) if is_sep(t) && toks.get(dots + 1) == Some(t) => {
            (Some(t.clone()), unit_with_sep[..p - 1].to_vec())
        }
        _ => (None, unit_with_sep),
    };
    // The last term, after the dots, fills the changing places.
    let mut j = dots + 1 + usize::from(sep.is_some());
    let mut lasts: Vec<Vec<Tok>> = Vec::new();
    for (i, t) in unit.iter().enumerate() {
        if steps.iter().any(|&(h, _, _)| h == i) {
            let braced =
                i > 0 && unit[i - 1] == Tok::LBrace && unit.get(i + 1) == Some(&Tok::RBrace);
            let group = if braced {
                // Up to the brace that closes the one before it.
                let mut depth = 0;
                let from = j;
                while j < toks.len() {
                    match toks[j] {
                        Tok::LBrace | Tok::LParen => depth += 1,
                        Tok::RBrace | Tok::RParen if depth == 0 => break,
                        Tok::RBrace | Tok::RParen => depth -= 1,
                        _ => {}
                    }
                    j += 1;
                }
                toks[from..j].to_vec()
            } else if matches!(toks.get(j), Some(Tok::LBrace | Tok::LParen)) {
                let end = group_end(toks, j).ok_or_else(cannot)?;
                let g = if toks[j] == Tok::LBrace {
                    toks[j + 1..end].to_vec()
                } else {
                    toks[j..=end].to_vec()
                };
                j = end + 1;
                g
            } else if sep.is_some() && i + 1 == unit.len() && toks.get(j).is_some() {
                // The whole last term, such as 2n in 2+4+...+2n.
                let from = j;
                let mut depth = 0;
                while let Some(t) = toks.get(j) {
                    match t {
                        Tok::LBrace | Tok::LParen | Tok::LBracket => depth += 1,
                        Tok::RBrace | Tok::RParen | Tok::RBracket if depth == 0 => break,
                        Tok::RBrace | Tok::RParen | Tok::RBracket => depth -= 1,
                        Tok::Plus | Tok::Minus | Tok::Comma | Tok::Eq if depth == 0 => break,
                        t if depth == 0 && Some(t) == sep.as_ref() => break,
                        _ => {}
                    }
                    j += 1;
                }
                toks[from..j].to_vec()
            } else if let Some(t) = toks.get(j) {
                j += 1;
                vec![t.clone()]
            } else {
                return Err("The ... needs a last term after it, such as 1+2+...+n".into());
            };
            if group.is_empty() {
                return Err(cannot());
            }
            lasts.push(group);
        } else if toks.get(j) == Some(t) {
            j += 1;
        } else {
            return Err(
                "The term after the ... should match the ones before it, such as x_1x_2...x_n"
                    .into(),
            );
        }
    }
    let holes = steps
        .iter()
        .zip(firsts)
        .zip(lasts)
        .map(|((&(at, _, step), first), last)| {
            let sub = |k: usize| k < at && unit[at - k] == Tok::Underscore;
            let name = (sub(1) || (sub(2) && unit[at - 1] == Tok::LBrace))
                && matches!(
                    unit.get(at.wrapping_sub(if sub(1) { 2 } else { 3 })),
                    Some(Tok::Letter(_))
                );
            Hole {
                at,
                first,
                step,
                last,
                name,
            }
        })
        .collect();
    Ok(Chain {
        start,
        end: j,
        unit,
        sep,
        holes,
    })
}

impl Chain {
    /// How many terms the chain has, when that is a number.
    pub fn count(&self) -> Option<usize> {
        let counts: Vec<usize> = self.holes.iter().filter_map(Hole::count).collect();
        counts.first().copied()
    }

    /// Term `k` (from 0), written out.
    pub fn term(&self, k: usize) -> Vec<Tok> {
        let mut t = self.unit.clone();
        for h in &self.holes {
            t[h.at] = Tok::Num(h.first + h.step * k as f64);
        }
        t
    }

    /// Every term, written out with its separators.
    fn written_out(&self, count: usize) -> Vec<Tok> {
        let mut out = Vec::new();
        for k in 0..count {
            if k > 0 {
                if let Some(s) = &self.sep {
                    out.push(s.clone());
                }
            }
            out.extend(self.term(k));
        }
        out
    }

    /// A sum or product over an index, for a chain whose length is unknown.
    fn indexed(&self, index: &str) -> Option<Vec<Tok>> {
        if self.holes.is_empty()
            || self.holes.iter().any(|h| h.name)
            || self.unit.contains(&Tok::Int)
        {
            return None;
        }
        let op = match self.sep {
            Some(Tok::Plus) => Tok::Sum,
            Some(Tok::Mul) | None => Tok::Prod,
            _ => return None,
        };
        let mut body = Vec::new();
        for (i, t) in self.unit.iter().enumerate() {
            match self.holes.iter().find(|h| h.at == i) {
                Some(h) => body.extend(h.value_tokens(index)),
                None => body.push(t.clone()),
            }
        }
        let mut out = vec![
            op,
            Tok::Underscore,
            Tok::LBrace,
            Tok::Letter(index.to_owned()),
            Tok::Eq,
            Tok::Num(1.0),
            Tok::RBrace,
            Tok::Caret,
            Tok::LBrace,
        ];
        out.extend(self.holes[0].count_tokens());
        out.extend([Tok::RBrace, Tok::LParen]);
        out.extend(body);
        out.push(Tok::RParen);
        Some(out)
    }
}

/// The index for a sum written with dots: a letter the line doesn't use.
fn free_index(toks: &[Tok]) -> &'static str {
    ["k", "j", "m", "l", "q"]
        .into_iter()
        .find(|c| !toks.iter().any(|t| matches!(t, Tok::Letter(l) if l == c)))
        .unwrap_or("k")
}

/// The `...` outside lists, in order.
pub fn dots_outside_lists(toks: &[Tok]) -> Vec<usize> {
    let mut depth = 0i32;
    let mut out = Vec::new();
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::LBracket => depth += 1,
            Tok::RBracket => depth -= 1,
            Tok::Dots if depth == 0 => out.push(i),
            _ => {}
        }
    }
    out
}

/// Writes out every chain whose length is known and turns sums and
/// products of unknown length into Σ and ∏. Gives `SYMBOLIC` for chains
/// only Solve can work with.
pub fn rewrite(mut toks: Vec<Tok>) -> Result<Vec<Tok>, String> {
    let mut symbolic = false;
    let mut skip = 0;
    loop {
        let all = dots_outside_lists(&toks);
        let Some(&at) = all.get(skip) else { break };
        let c = chain(&toks, at)?;
        // The same term repeated takes its length from another chain, as
        // \int_0^1...\int_0^1 does from dx_1...dx_n.
        let count = if c.holes.is_empty() {
            all.iter()
                .rev()
                .filter(|&&d| d != at)
                .find_map(|&d| chain(&toks, d).ok().and_then(|o| o.count()))
        } else {
            c.count()
        };
        let replacement = match count {
            Some(n) if n <= MOST => c.written_out(n),
            Some(_) => return Err(format!("A ... can stand for at most {MOST} terms")),
            None => match c.indexed(free_index(&toks)) {
                Some(t) => t,
                None => {
                    // Left as written; later dots are still checked.
                    symbolic = true;
                    skip += 1;
                    continue;
                }
            },
        };
        toks.splice(c.start..c.end, replacement);
    }
    if symbolic {
        Err(SYMBOLIC.into())
    } else {
        Ok(toks)
    }
}

/// Whether `latex` has a chain only Solve can work with.
pub fn is_symbolic(latex: &str) -> bool {
    crate::lexer::tokenize(latex).is_ok_and(|t| rewrite(t).err().as_deref() == Some(SYMBOLIC))
}

/// `latex` with the letter `name` (not inside a command such as `\ln`)
/// replaced by `value`, so `x_1...x_n` with n = 3 can be written out.
pub fn set_letter(latex: &str, name: char, value: &str) -> String {
    let mut out = String::new();
    let mut chars = latex.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(c);
            while let Some(&d) = chars.peek() {
                if !d.is_ascii_alphabetic() {
                    break;
                }
                out.push(d);
                chars.next();
            }
            // \operatorname{...} names are words, not letters.
            if out.ends_with("\\operatorname") || out.ends_with("\\text") {
                for d in chars.by_ref() {
                    out.push(d);
                    if d == '}' {
                        break;
                    }
                }
            }
        } else if c == name && out.ends_with('{') && chars.peek() == Some(&'}') {
            out.push_str(value);
        } else if c == name {
            out.push_str(&format!("{{{value}}}"));
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::tokenize;

    fn value(latex: &str) -> f64 {
        let rows = [crate::RowInput {
            id: 0,
            latex: latex.into(),
            ..Default::default()
        }];
        match crate::analyze(&rows)[0].value {
            Some(crate::Value::Number(v)) => v,
            ref other => panic!("{latex}: {other:?} {:?}", crate::analyze(&rows)[0].error),
        }
    }

    fn values(rows: &[&str]) -> f64 {
        let rows: Vec<crate::RowInput> = rows
            .iter()
            .enumerate()
            .map(|(i, l)| crate::RowInput {
                id: i as u64,
                latex: l.to_string(),
                ..Default::default()
            })
            .collect();
        let out = crate::analyze(&rows);
        match out.last().unwrap().value {
            Some(crate::Value::Number(v)) => v,
            ref other => panic!("{other:?} {:?}", out.last().unwrap().error),
        }
    }

    #[test]
    fn known_counts_are_written_out() {
        assert_eq!(value("1+2+...+10"), 55.0);
        assert_eq!(value("1+2+\\ldots+10"), 55.0);
        assert_eq!(value("2\\cdot 4\\cdot ...\\cdot 8"), 384.0);
        assert_eq!(
            value("\\frac{1}{1}+\\frac{1}{2}+...+\\frac{1}{4}"),
            1.0 + 0.5 + 1.0 / 3.0 + 0.25
        );
        assert_eq!(value("1^2+3^2+...+7^2"), 1.0 + 9.0 + 25.0 + 49.0);
        // Names with subscripts, and defined values for them.
        assert_eq!(values(&["x_1=1", "x_2=2", "x_3=5", "x_1x_2...x_3"]), 10.0);
        assert_eq!(values(&["a_1=1", "a_2=2", "a_3=4", "a_1+a_2+...+a_3"]), 7.0);
        // Lists still use ... as before.
        assert_eq!(
            values(&[
                "L=\\left[1...4\\right]",
                "\\operatorname{total}\\left(L\\right)"
            ]),
            10.0
        );
    }

    #[test]
    fn unknown_counts_become_sums_and_products() {
        assert_eq!(values(&["n=10", "1+2+...+n"]), 55.0);
        assert_eq!(values(&["n=5", "1\\cdot 2\\cdot ...\\cdot n"]), 120.0);
        assert_eq!(values(&["n=4", "1^2+2^2+...+n^2"]), 30.0);
        assert_eq!(values(&["n=3", "2+4+...+2n"]), 12.0);
        assert_eq!(values(&["n=4", "\\frac{1}{1\\cdot 2}+\\frac{1}{2\\cdot 3}+...+\\frac{1}{n\\cdot\\left(n+1\\right)}"]), 0.8);
        assert_eq!(
            values(&["f\\left(n\\right)=1+2+...+n", "f\\left(4\\right)"]),
            10.0
        );
    }

    #[test]
    fn n_fold_integrals_are_left_to_solve() {
        let i = "I_n=\\int_1^2\\int_2^3...\\int_n^{n+1}\\frac{\\ln\\left(x_1x_2...x_n\\right)}{\\left(x_1x_2...x_n\\right)^2}dx_1dx_2...dx_n";
        assert!(is_symbolic(i));
        let rows = [crate::RowInput {
            id: 0,
            latex: i.into(),
            ..Default::default()
        }];
        let r = &crate::analyze(&rows)[0];
        assert!(r.error.is_none(), "{:?}", r.error);
        assert!(is_symbolic("x_1+x_2+...+x_n"));
        assert!(!is_symbolic("1+2+...+n"));
        // With n given, it is a plain double integral.
        let two = set_letter(i.split_once('=').unwrap().1, 'n', "2");
        let v = value(&two);
        assert!((v - 0.099_089_148_648_639_26).abs() < 1e-9, "{v}");
    }

    #[test]
    fn chains_read_their_pattern() {
        let t = tokenize("\\int_1^2\\int_2^3...\\int_n^{n+1}").unwrap();
        let at = t.iter().position(|t| *t == Tok::Dots).unwrap();
        let c = chain(&t, at).unwrap();
        assert_eq!((c.start, c.end), (0, t.len()));
        assert_eq!(c.holes.len(), 2);
        assert_eq!(c.holes[0].last, vec![Tok::Letter("n".into())]);
        assert!(!c.holes[0].name);
        let t = tokenize("x_1x_2...x_n").unwrap();
        let c = chain(&t, 6).unwrap();
        assert!(c.holes[0].name);
        assert!(chain(&tokenize("x...y").unwrap(), 1).is_err());
        assert_eq!(
            set_letter("\\ln\\left(x_n\\right)+n", 'n', "3"),
            "\\ln\\left(x_{3}\\right)+{3}"
        );
    }
}
