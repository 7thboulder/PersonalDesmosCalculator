//! Subscripts that hold expressions, like `H_{2n-1}`.
//!
//! A row such as `H_n = \sum_{k=1}^{n} 1/k`, whose subscript is one letter,
//! also says what `H` means with any subscript: `H_{2n-1}`, `H_5` and `H_m`
//! are that row worked out with n set to 2n-1, 5 and m. A name some row
//! sets itself, like `H_4 = 7`, is still that row.
//!
//! The parser reads a subscript that holds an expression as a call to a
//! function named with a trailing underscore (`H_{2n-1}` is `H_(2n-1)`), a
//! name no row can write, and this module builds those functions.

use std::collections::HashMap;

use crate::ast::{Expr, Relation, Statement};

/// The functions that rows like `H_n = …` define, keyed by their name with
/// the trailing underscore (`H_`): the subscript letter and the body.
/// A letter with two such rows (`H_n` and `H_m`) defines nothing, and nor
/// does a row that uses itself, like `a_n = 2a_{n-1}`.
pub fn functions<'a>(
    defs: impl IntoIterator<Item = (&'a str, &'a Expr)>,
) -> HashMap<String, (String, Expr)> {
    let mut found: HashMap<String, Vec<(String, Expr)>> = HashMap::new();
    for (name, value) in defs {
        if let Some((key, param)) = indexed(name) {
            found.entry(key).or_default().push((param, value.clone()));
        }
    }
    found
        .into_iter()
        .filter_map(|(key, mut rows)| {
            let (param, body) = rows.pop().filter(|_| rows.is_empty())?;
            (!body.mentions(&key)).then_some((key, (param, body)))
        })
        .collect()
}

/// `H_n` as the function it defines, `H_`, and its subscript letter.
pub fn indexed(name: &str) -> Option<(String, String)> {
    let (letter, sub) = name.split_once('_')?;
    let one_letter = sub.len() == 1 && sub.chars().all(|c| c.is_ascii_alphabetic());
    (!letter.is_empty() && one_letter).then(|| (format!("{letter}_"), sub.to_owned()))
}

/// The names a statement defines as variables and their values, for
/// [`functions`].
pub fn var_definition(statement: &Statement) -> Option<(&str, &Expr)> {
    match statement {
        Statement::Relation(Expr::Var(name), Relation::Eq, value) => Some((name, value)),
        _ => None,
    }
}

/// `H_5` or `H_m` where no row defines that name but `H_` is a function:
/// the call `H_(5)` or `H_(m)`.
pub fn as_call(name: &str, is_function: &dyn Fn(&str) -> bool) -> Option<Expr> {
    let (letter, sub) = name.split_once('_')?;
    if letter.is_empty() || sub.is_empty() || name.contains('\'') {
        return None;
    }
    let key = format!("{letter}_");
    if !is_function(&key) {
        return None;
    }
    // The subscript is letters and digits, so `2n` is 2 times n.
    let Ok(Statement::Expr(index)) = crate::parse(sub) else {
        return None;
    };
    Some(Expr::Apply(key, vec![index]))
}

/// Rewrites every variable that [`as_call`] turns into a call, leaving the
/// names `defined` says some row sets.
pub fn rewrite(
    e: Expr,
    defined: &dyn Fn(&str) -> bool,
    is_function: &dyn Fn(&str) -> bool,
) -> Expr {
    crate::units::rewrite(e, &mut |e| match e {
        Expr::Var(name) if !defined(&name) => match as_call(&name, is_function) {
            Some(call) => Ok(call),
            None => Err(Expr::Var(name)),
        },
        e => Err(e),
    })
}

/// Applies [`rewrite`] to each side of a statement, except the name a
/// definition sets.
pub fn rewrite_statement(
    statement: &mut Statement,
    defined: &dyn Fn(&str) -> bool,
    is_function: &dyn Fn(&str) -> bool,
) {
    let fix = |e: &mut Expr| {
        let taken = std::mem::replace(e, Expr::Num(0.0));
        *e = rewrite(taken, defined, is_function);
    };
    match statement {
        Statement::Expr(e) => fix(e),
        Statement::Relation(a, rel, b) => {
            let defines = *rel == Relation::Eq && matches!(a, Expr::Var(_) | Expr::Apply(..));
            if !defines {
                fix(a);
            }
            fix(b);
        }
        Statement::Regression(a, b) => {
            fix(a);
            fix(b);
        }
        Statement::Chain(c) => c.operands.iter_mut().for_each(fix),
    }
}

/// The message for `H_{…}` when no row says what H with a subscript is.
pub fn undefined(key: &str) -> String {
    let letter = key.trim_end_matches('_');
    format!(
        "{letter} with that subscript needs a row like {letter}_n = … that says what {letter}_n is"
    )
}
