//! Splits the LaTeX that MathQuill produces into tokens.

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Num(f64),
    /// A single letter, or a Greek letter or constant spelled as a command
    /// (`\theta` becomes `theta`).
    Letter(String),
    /// A named operator: `\sin` or `\operatorname{floor}`.
    OpName(String),
    Frac,
    Sqrt,
    LParen,
    RParen,
    AbsOpen,
    AbsClose,
    /// A bare `|` that MathQuill did not mark as left or right.
    Pipe,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    /// `\left\{` and `\right\}`, used by piecewise definitions.
    LCurly,
    RCurly,
    Plus,
    Minus,
    Mul,
    Div,
    Caret,
    Underscore,
    Comma,
    Colon,
    Bang,
    Prime,
    Dots,
    /// `.` before a letter, as in `p.x`.
    Dot,
    Eq,
    Lt,
    Gt,
    Le,
    Ge,
    Sum,
    Prod,
    Int,
    /// A command this calculator does not understand yet.
    Unsupported(String),
}

const GREEK: &[&str] = &[
    "alpha",
    "beta",
    "gamma",
    "delta",
    "epsilon",
    "varepsilon",
    "zeta",
    "eta",
    "theta",
    "vartheta",
    "iota",
    "kappa",
    "lambda",
    "mu",
    "nu",
    "xi",
    "pi",
    "rho",
    "sigma",
    "tau",
    "upsilon",
    "phi",
    "varphi",
    "chi",
    "psi",
    "omega",
    "Gamma",
    "Delta",
    "Theta",
    "Lambda",
    "Xi",
    "Pi",
    "Sigma",
    "Upsilon",
    "Phi",
    "Psi",
    "Omega",
    "infty",
];

pub fn tokenize(src: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = src.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '~' => i += 1,
            '0'..='9' | '.' => {
                if c == '.'
                    && chars
                        .get(i + 1)
                        .is_some_and(|n| n.is_ascii_alphabetic() || *n == '\\')
                {
                    toks.push(Tok::Dot);
                    i += 1;
                    continue;
                }
                if c == '.' && chars.get(i + 1) == Some(&'.') {
                    while chars.get(i) == Some(&'.') {
                        i += 1;
                    }
                    toks.push(Tok::Dots);
                    continue;
                }
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    // Stop before `..`, which is a range, not a decimal point.
                    if chars[i] == '.' && chars.get(i + 1) == Some(&'.') {
                        break;
                    }
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                let value = text
                    .parse::<f64>()
                    .map_err(|_| format!("'{text}' is not a number"))?;
                toks.push(Tok::Num(value));
            }
            'a'..='z' | 'A'..='Z' => {
                toks.push(Tok::Letter(c.to_string()));
                i += 1;
            }
            '\\' => {
                i += 1;
                let start = i;
                while i < chars.len() && chars[i].is_ascii_alphabetic() {
                    i += 1;
                }
                if start == i {
                    // A one-character command such as `\{`, `\ ` or `\,`.
                    let symbol = chars.get(i).copied().unwrap_or(' ');
                    i += 1;
                    match symbol {
                        '{' => toks.push(Tok::LCurly),
                        '}' => toks.push(Tok::RCurly),
                        ' ' | ',' | ';' | ':' | '!' => {}
                        '%' => toks.push(Tok::Unsupported("%".into())),
                        other => toks.push(Tok::Unsupported(other.to_string())),
                    }
                    continue;
                }
                let name: String = chars[start..i].iter().collect();
                i = command(&name, &chars, i, &mut toks)?;
            }
            '(' => push(&mut toks, Tok::LParen, &mut i),
            ')' => push(&mut toks, Tok::RParen, &mut i),
            '{' => push(&mut toks, Tok::LBrace, &mut i),
            '}' => push(&mut toks, Tok::RBrace, &mut i),
            '[' => push(&mut toks, Tok::LBracket, &mut i),
            ']' => push(&mut toks, Tok::RBracket, &mut i),
            '|' => push(&mut toks, Tok::Pipe, &mut i),
            '+' => push(&mut toks, Tok::Plus, &mut i),
            '-' => push(&mut toks, Tok::Minus, &mut i),
            '*' => push(&mut toks, Tok::Mul, &mut i),
            '/' => push(&mut toks, Tok::Div, &mut i),
            '^' => push(&mut toks, Tok::Caret, &mut i),
            '_' => push(&mut toks, Tok::Underscore, &mut i),
            ',' => push(&mut toks, Tok::Comma, &mut i),
            ':' => push(&mut toks, Tok::Colon, &mut i),
            '!' => push(&mut toks, Tok::Bang, &mut i),
            '\'' => push(&mut toks, Tok::Prime, &mut i),
            '=' => push(&mut toks, Tok::Eq, &mut i),
            '<' => push(&mut toks, Tok::Lt, &mut i),
            '>' => push(&mut toks, Tok::Gt, &mut i),
            other => return Err(format!("unexpected character '{other}'")),
        }
    }
    Ok(toks)
}

fn push(toks: &mut Vec<Tok>, tok: Tok, i: &mut usize) {
    toks.push(tok);
    *i += 1;
}

/// Handles `\name`, returning the index after everything it consumed.
fn command(name: &str, chars: &[char], mut i: usize, toks: &mut Vec<Tok>) -> Result<usize, String> {
    let tok = match name {
        "left" | "right" => {
            let is_left = name == "left";
            // Skip spaces, then read the delimiter that follows.
            while chars.get(i) == Some(&' ') {
                i += 1;
            }
            let delim = chars.get(i).copied().ok_or("unfinished bracket")?;
            i += 1;
            match (delim, is_left) {
                ('(', _) => Tok::LParen,
                (')', _) => Tok::RParen,
                ('[', _) => Tok::LBracket,
                (']', _) => Tok::RBracket,
                ('|', true) => Tok::AbsOpen,
                ('|', false) => Tok::AbsClose,
                ('.', _) => return Ok(i),
                ('\\', _) => {
                    let next = chars.get(i).copied();
                    i += 1;
                    match (next, is_left) {
                        (Some('{'), _) => Tok::LCurly,
                        (Some('}'), _) => Tok::RCurly,
                        _ => {
                            // `\left\langle` and friends: read the word and give up.
                            while i < chars.len() && chars[i].is_ascii_alphabetic() {
                                i += 1;
                            }
                            Tok::Unsupported("bracket".into())
                        }
                    }
                }
                _ => Tok::Unsupported("bracket".into()),
            }
        }
        "operatorname" => {
            // \operatorname{name}
            let mut j = i;
            while chars.get(j) == Some(&' ') {
                j += 1;
            }
            if chars.get(j) != Some(&'{') {
                return Err("\\operatorname needs a name".into());
            }
            j += 1;
            let start = j;
            while j < chars.len() && chars[j] != '}' {
                j += 1;
            }
            let op: String = chars[start..j].iter().collect();
            i = j + 1;
            Tok::OpName(op)
        }
        "frac" | "dfrac" | "tfrac" => Tok::Frac,
        "sqrt" => Tok::Sqrt,
        "cdot" | "times" | "ast" => Tok::Mul,
        "div" => Tok::Div,
        "le" | "leq" => Tok::Le,
        "ge" | "geq" => Tok::Ge,
        "lt" => Tok::Lt,
        "gt" => Tok::Gt,
        "ldots" | "dots" | "cdots" => Tok::Dots,
        "prime" => Tok::Prime,
        "sum" => Tok::Sum,
        "prod" => Tok::Prod,
        "int" => Tok::Int,
        "mathrm" | "text" | "mathit" => {
            return Err(format!("text (\\{name}) is not supported"));
        }
        n if GREEK.contains(&n) => Tok::Letter(n.to_string()),
        n if crate::builtins::is_operator_name(n) => Tok::OpName(n.to_string()),
        other => Tok::Unsupported(other.to_string()),
    };
    toks.push(tok);
    Ok(i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Tok::*;

    #[test]
    fn tokenizes_mathquill_output() {
        let toks = tokenize("y=\\sin\\left(x\\right)+\\frac{3}{4}").unwrap();
        assert_eq!(
            toks,
            vec![
                Letter("y".into()),
                Eq,
                OpName("sin".into()),
                LParen,
                Letter("x".into()),
                RParen,
                Plus,
                Frac,
                LBrace,
                Num(3.0),
                RBrace,
                LBrace,
                Num(4.0),
                RBrace
            ]
        );
    }

    #[test]
    fn operatorname_greek_and_abs() {
        let toks = tokenize("\\operatorname{floor}\\left|\\theta \\right|\\le 2.5").unwrap();
        assert_eq!(
            toks,
            vec![
                OpName("floor".into()),
                AbsOpen,
                Letter("theta".into()),
                AbsClose,
                Le,
                Num(2.5)
            ]
        );
    }
}
