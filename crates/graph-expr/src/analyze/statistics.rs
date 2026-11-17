//! Statistics in rows, as Desmos has them: distributions such as
//! `normaldist(0, 1)` (drawn as their density, with cumulative
//! probabilities shaded on request), `D.pdf(x)`, `D.cdf(a, b)` and
//! `D.random(n)`, random numbers and shuffles, and the rows that show a
//! plot or a table: `histogram(L)`, `dotplot(L)`, `boxplot(L)`,
//! `stats(L)`, `ttest(L, μ₀)` and the other tests and intervals.

use super::*;
use crate::ast::{Method, StatRow};
use crate::stats::{self, BarHeight, BinAlign, Dist, DistKind, DistOp, Tail};

/// The settings a statistics row shows under it, as the panel sends them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatOptions {
    /// For a distribution: the min and max of "Find Cumulative
    /// Probability", as LaTeX, while it is on. Empty means unbounded.
    pub cdf: Option<[String; 2]>,
    /// For a histogram or dot plot.
    pub align: BinAlign,
    pub height: BarHeight,
    /// For a test: which side the alternative is on.
    pub tail: Tail,
    /// For a box plot: where it sits up the y axis and how tall it is, as
    /// LaTeX (empty for the defaults, 1 and 1).
    pub offset: String,
    pub box_height: String,
    /// For a box plot: let the whiskers reach the minimum and maximum
    /// rather than marking outliers as dots.
    pub whiskers_to_ends: bool,
}

/// Which kind of statistics row this is, for the panel's settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatKind {
    Distribution {
        discrete: bool,
    },
    Histogram,
    Dotplot,
    Boxplot,
    /// A table of results: a summary, a test or an interval.
    Table,
}

/// What a statistics row shows besides what it draws.
#[derive(Clone, Debug, PartialEq)]
pub struct StatsOut {
    pub kind: StatKind,
    pub table: Option<stats::Table>,
    /// The probability "Find Cumulative Probability" works out.
    pub probability: Option<f64>,
    /// What is wrong with the min or max of the cumulative probability.
    pub cdf_error: Option<String>,
    /// What is wrong with a box plot's offset or height.
    pub box_error: Option<String>,
}

impl StatsOut {
    fn new(kind: StatKind) -> StatsOut {
        StatsOut {
            kind,
            table: None,
            probability: None,
            cdf_error: None,
            box_error: None,
        }
    }
}

/// The seed every random draw starts from; the panel's re-randomize
/// button changes it.
static GRAPH_SEED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Sets the seed random draws start from, so they all change at once.
pub fn set_random_seed(seed: u64) {
    GRAPH_SEED.store(seed, std::sync::atomic::Ordering::Relaxed);
}

/// The most values `random` and `D.random` give at once.
const MAX_DRAWS: f64 = 1_000_000.0;
/// The most bars a discrete distribution draws.
const MAX_BARS: f64 = 5_000.0;
/// How many points trace the top of a shaded stretch of density.
const SHADE_POINTS: usize = 400;

fn fold_const(l: &Low) -> Option<f64> {
    match l.clone().fold() {
        Low::Const(v) => Some(v),
        _ => None,
    }
}

/// A value's numbers, when every element is one.
fn nums(v: &Val) -> Option<Vec<Low>> {
    v.items
        .iter()
        .map(|i| match i {
            Item::Num(l) => Some(l.clone()),
            _ => None,
        })
        .collect()
}

/// A value's numbers as constants.
fn consts(v: &Val) -> Option<Vec<f64>> {
    nums(v)?.iter().map(fold_const).collect()
}

/// The one distribution a value holds.
fn dist_of(v: &Val) -> Option<(DistKind, Vec<Low>)> {
    match v.items.as_slice() {
        [Item::Dist(kind, params)] if !v.list => Some((*kind, params.clone())),
        _ => None,
    }
}

impl Env {
    /// The next seed for a random draw in the row or variable being
    /// worked out.
    fn next_seed(&self) -> u64 {
        let n = self.draws.get();
        self.draws.set(n + 1);
        stats::seed_of(&[
            GRAPH_SEED.load(std::sync::atomic::Ordering::Relaxed),
            self.salt.get(),
            n,
        ])
    }

    /// Sets what random draws are told apart by: the row, or the variable,
    /// being worked out. Returns the old setting, to put back.
    pub(super) fn draw_for(&self, salt: u64) -> (u64, u64) {
        let old = (self.salt.get(), self.draws.get());
        self.salt.set(salt);
        self.draws.set(0);
        old
    }

    pub(super) fn restore_draws(&self, old: (u64, u64)) {
        self.salt.set(old.0);
        self.draws.set(old.1);
    }

    /// `normaldist(μ, σ)` and the others: parameters left out take their
    /// defaults, and constant ones are checked.
    pub(super) fn distribution(
        &self,
        kind: DistKind,
        args: Vec<Val>,
        problems: &mut Problems,
    ) -> Val {
        let usage = || format!("Try {}", kind.usage());
        if args.len() > kind.params() || args.iter().any(|a| a.list) {
            problems.error(usage());
            return Val::nan();
        }
        let mut params = Vec::new();
        for a in &args {
            match a.items.as_slice() {
                [Item::Num(l)] => params.push(l.clone()),
                _ => {
                    problems.error(usage());
                    return Val::nan();
                }
            }
        }
        // Fill in the defaults by checking with them.
        let given: Option<Vec<f64>> = params.iter().map(fold_const).collect();
        match given {
            Some(values) => match Dist::new(kind, &values) {
                Ok(d) => {
                    params = d.p[..kind.params()]
                        .iter()
                        .map(|v| Low::Const(*v))
                        .collect();
                }
                Err(e) => {
                    problems.error(e);
                    return Val::nan();
                }
            },
            None => {
                // Parameters that move with x: only the count is checked.
                if params.len() != kind.params() {
                    problems.error(usage());
                    return Val::nan();
                }
            }
        }
        Val {
            items: vec![Item::Dist(kind, params)],
            list: false,
        }
    }

    /// `D.pdf(x)`, `D.cdf(x)`, `D.cdf(a, b)` and `D.random(n, seed)`.
    pub(super) fn method(&self, m: Method, mut args: Vec<Val>, problems: &mut Problems) -> Val {
        let base = args.remove(0);
        let Some((kind, params)) = dist_of(&base) else {
            problems.error(format!(
                ".{} follows a distribution, such as normaldist(0, 1).{}(x)",
                m.name(),
                m.name()
            ));
            return Val::nan();
        };
        let call = |op: DistOp, rest: Vec<Low>| {
            let mut all = params.clone();
            all.extend(rest);
            Item::Num(shallow(Low::Call(Builtin::Dist(kind, op), all)))
        };
        match m {
            Method::Pdf => {
                if args.len() != 1 {
                    problems.error("Try D.pdf(x)");
                    return Val::nan();
                }
                self.broadcast(&args, problems, |items| match items[0] {
                    Item::Num(x) => Ok(call(DistOp::Pdf, vec![x.clone()])),
                    _ => Err("D.pdf(x) takes a number".into()),
                })
            }
            Method::Cdf => {
                let op = match args.len() {
                    1 => DistOp::Cdf,
                    2 => DistOp::CdfBetween,
                    _ => {
                        problems
                            .error("Try D.cdf(x) for P(X ≤ x), or D.cdf(a, b) for P(a ≤ X ≤ b)");
                        return Val::nan();
                    }
                };
                self.broadcast(&args, problems, |items| {
                    let mut rest = Vec::new();
                    for i in items {
                        match i {
                            Item::Num(x) => rest.push(x.clone()),
                            _ => return Err("D.cdf takes numbers".to_string()),
                        }
                    }
                    Ok(call(op, rest))
                })
            }
            Method::Random => {
                let Some(params) = params.iter().map(fold_const).collect::<Option<Vec<f64>>>()
                else {
                    problems.error("A distribution to draw from can't depend on x or y");
                    return Val::nan();
                };
                let Some(d) = Dist::from_args(kind, &params) else {
                    problems.error(format!("Try {}", kind.usage()));
                    return Val::nan();
                };
                self.draw(args, problems, |n, rng| d.sample(n, rng))
            }
        }
    }

    /// Draws `n` values (args: optional count, then optional seed), as one
    /// number when no count is given.
    fn draw(
        &self,
        args: Vec<Val>,
        problems: &mut Problems,
        sample: impl Fn(usize, &mut stats::Rng) -> Vec<f64>,
    ) -> Val {
        if args.len() > 2 {
            problems.error("Try random(n), or random(n, seed) for the same draw every time");
            return Val::nan();
        }
        let mut numbers = Vec::new();
        for a in &args {
            match consts(a).as_deref() {
                Some([v]) if !a.list => numbers.push(*v),
                _ => {
                    problems.error("The count and seed of random draws must be single numbers");
                    return Val::nan();
                }
            }
        }
        let seed = match numbers.get(1) {
            Some(s) => stats::seed_of(&[s.to_bits()]),
            None => self.next_seed(),
        };
        let mut rng = stats::Rng::new(seed);
        match numbers.first() {
            None => Val::num(Low::Const(sample(1, &mut rng)[0])),
            Some(n) if *n >= 0.0 && *n <= MAX_DRAWS => {
                let values = sample(n.round() as usize, &mut rng);
                Val::list(
                    values
                        .into_iter()
                        .map(|v| Item::Num(Low::Const(v)))
                        .collect(),
                )
            }
            Some(_) => {
                problems.error("The number of random values must be 0 to 1,000,000");
                Val::nan()
            }
        }
    }

    /// `random()`, `random(n)`, `random(n, seed)`, `random(L)`,
    /// `random(L, n)` and `random(D, n)`.
    pub(super) fn random(&self, mut args: Vec<Val>, problems: &mut Problems) -> Val {
        let first = args.first();
        if let Some((kind, params)) = first.and_then(dist_of) {
            args.remove(0);
            let d = params
                .iter()
                .map(fold_const)
                .collect::<Option<Vec<f64>>>()
                .and_then(|p| Dist::from_args(kind, &p));
            let Some(d) = d else {
                problems.error(format!("Try {}", kind.usage()));
                return Val::nan();
            };
            return self.draw(args, problems, |n, rng| d.sample(n, rng));
        }
        if first.is_some_and(|f| f.list) {
            // Elements picked at random, with replacement.
            let list = args.remove(0);
            if list.items.is_empty() {
                return Val::list(Vec::new());
            }
            let picked = std::cell::RefCell::new(Vec::new());
            let len = list.items.len();
            let one = args.is_empty();
            let val = self.draw(args, problems, |n, rng| {
                let mut out = Vec::with_capacity(n);
                for _ in 0..n {
                    let k = rng.below(len);
                    picked.borrow_mut().push(k);
                    out.push(0.0);
                }
                out
            });
            let picked = picked.into_inner();
            let items: Vec<Item> = picked.iter().map(|k| list.items[*k].clone()).collect();
            if val.items.len() != items.len() {
                return val;
            }
            return if one {
                Val { items, list: false }
            } else {
                Val::list(items)
            };
        }
        self.draw(args, problems, |n, rng| {
            (0..n).map(|_| rng.next()).collect()
        })
    }

    /// `shuffle(L)` and `shuffle(L, seed)`.
    pub(super) fn shuffle(&self, args: Vec<Val>, problems: &mut Problems) -> Val {
        if args.is_empty() || args.len() > 2 {
            problems.error("Try shuffle(L), or shuffle(L, seed)");
            return Val::nan();
        }
        let seed = match args.get(1).map(consts) {
            Some(Some(s)) if s.len() == 1 => stats::seed_of(&[s[0].to_bits()]),
            Some(_) => {
                problems.error("A shuffle's seed must be a number");
                return Val::nan();
            }
            None => self.next_seed(),
        };
        let mut rng = stats::Rng::new(seed);
        let mut items = args[0].items.clone();
        // Fisher–Yates.
        for i in (1..items.len()).rev() {
            let j = rng.below(i + 1);
            items.swap(i, j);
        }
        Val::list(items)
    }

    /// `freq(L, F)`: each value of L as many times as F says.
    pub(super) fn freq(&self, args: Vec<Val>, problems: &mut Problems) -> Val {
        let [values, counts] = args.as_slice() else {
            problems.error("Try freq(L, F): the values, then how often each one comes up");
            return Val::nan();
        };
        let (Some(values), Some(counts)) = (consts(values), consts(counts)) else {
            problems.error("freq(L, F) needs lists of numbers that don't depend on x or y");
            return Val::nan();
        };
        match stats::expand(&values, &counts) {
            Ok(v) => Val::list(v.into_iter().map(|v| Item::Num(Low::Const(v))).collect()),
            Err(e) => {
                problems.error(e);
                Val::nan()
            }
        }
    }

    /// `mean(D)`, `median(D)`, `stdev(D)`, `var(D)` and `quantile(D, p)`.
    pub(super) fn call_on_dist(&self, f: Builtin, args: Vec<Val>, problems: &mut Problems) -> Val {
        let Some((kind, params)) = args.first().and_then(dist_of) else {
            problems.error(format!(
                "Function '{}' can't take a distribution here",
                f.name()
            ));
            return Val::nan();
        };
        let call = |op: DistOp, rest: Vec<Low>| {
            let mut all = params.clone();
            all.extend(rest);
            shallow(Low::Call(Builtin::Dist(kind, op), all))
        };
        let only = |problems: &mut Problems| {
            if args.len() != 1 {
                problems.error(format!("Try {}(D)", f.name()));
                return false;
            }
            true
        };
        match f {
            Builtin::Mean if only(problems) => Val::num(call(DistOp::Mean, vec![])),
            Builtin::Var | Builtin::Varp if only(problems) => Val::num(call(DistOp::Var, vec![])),
            Builtin::Stdev | Builtin::Stdevp if only(problems) => Val::num(shallow(Low::Call(
                Builtin::Sqrt,
                vec![call(DistOp::Var, vec![])],
            ))),
            Builtin::Median if only(problems) => {
                Val::num(call(DistOp::Quantile, vec![Low::Const(0.5)]))
            }
            Builtin::Quantile if args.len() == 2 => {
                self.broadcast(&args[1..], problems, |items| match items[0] {
                    Item::Num(p) => Ok(Item::Num(call(DistOp::Quantile, vec![p.clone()]))),
                    _ => Err("quantile(D, p) takes a number p from 0 to 1".into()),
                })
            }
            Builtin::Mean
            | Builtin::Var
            | Builtin::Varp
            | Builtin::Stdev
            | Builtin::Stdevp
            | Builtin::Median => Val::nan(),
            _ => {
                problems.error(format!(
                    "Function '{}' can't take a distribution; try mean, median, stdev, var or quantile",
                    f.name()
                ));
                Val::nan()
            }
        }
    }

    /// `cov(L₁, L₂)`, `corr`, `spearman`, `quantile(L, p)`,
    /// `quartile(L, q)` and `tscore(L, μ₀)`.
    pub(super) fn call_on_lists(&self, f: Builtin, args: Vec<Val>, problems: &mut Problems) -> Val {
        let name = f.name();
        let Some(first) = args.first().and_then(nums) else {
            problems.error(format!("Function '{name}' takes a list of numbers"));
            return Val::nan();
        };
        match f {
            Builtin::Cov | Builtin::Covp | Builtin::Corr | Builtin::Spearman => {
                let second = match args.get(1).and_then(nums) {
                    Some(b) if args.len() == 2 => b,
                    _ => {
                        problems.error(format!("Try {name}(L_1, L_2), with two lists"));
                        return Val::nan();
                    }
                };
                if first.len() != second.len() {
                    problems.error(format!(
                        "{name}(L_1, L_2) needs two lists of the same length"
                    ));
                    return Val::nan();
                }
                let mut all = first;
                all.extend(second);
                Val::num(shallow(Low::Call(f, all)))
            }
            _ => {
                if args.len() > 2 {
                    problems.error(format!("Function '{name}' takes 2 arguments"));
                    return Val::nan();
                }
                // tscore(L) is against 0.
                let last = match args.get(1) {
                    Some(v) => v.clone(),
                    None if f == Builtin::TScore => Val::num(Low::Const(0.0)),
                    None => {
                        problems.error(format!(
                            "Try {name}(L, {})",
                            if f == Builtin::Quartile { "q" } else { "p" }
                        ));
                        return Val::nan();
                    }
                };
                self.broadcast(&[last], problems, |items| match items[0] {
                    Item::Num(q) => {
                        let mut all = first.clone();
                        all.push(q.clone());
                        Ok(Item::Num(shallow(Low::Call(f, all))))
                    }
                    _ => Err(format!("Function '{name}' takes a number last")),
                })
            }
        }
    }

    /// A list of numbers that doesn't depend on x or y, from a row's
    /// argument. `None` when it reads an undefined name.
    fn data(&mut self, e: &Expr, problems: &mut Problems) -> Result<Option<Vec<f64>>, String> {
        Ok(match self.constant(e, problems) {
            Some(Value::Numbers(v)) => Some(v),
            Some(Value::Number(v)) => Some(vec![v]),
            None => None,
            _ => return Err("This takes a list of numbers".into()),
        })
    }

    /// One number from a row's argument, or `default` when it is left out.
    fn number_arg(
        &mut self,
        e: Option<&Expr>,
        default: Option<f64>,
        what: &str,
        problems: &mut Problems,
    ) -> Result<Option<f64>, String> {
        match e {
            None => default.map(Some).ok_or_else(|| format!("Give {what}")),
            Some(e) => match self.constant(e, problems) {
                Some(Value::Number(v)) => Ok(Some(v)),
                None => Ok(None),
                _ => Err(format!("{what} must be a number")),
            },
        }
    }

    /// A number typed into one of a row's settings, such as a box plot's
    /// offset. Empty gives `default`.
    fn setting(
        &mut self,
        latex: &str,
        default: f64,
        problems: &mut Problems,
    ) -> Result<f64, String> {
        if latex.trim().is_empty() {
            return Ok(default);
        }
        let e = match parse(latex) {
            Ok(Statement::Expr(e)) => e,
            Ok(_) => return Err("Type a number".into()),
            Err(e) => return Err(e),
        };
        match self.constant(&e, problems) {
            Some(Value::Number(v)) if !v.is_nan() => Ok(v),
            None => Ok(f64::NAN),
            _ => Err("Type a number".into()),
        }
    }

    /// A row of its own: a plot of a list, a summary, a test or an
    /// interval.
    pub(super) fn stat_row(&mut self, input: &RowInput, r: StatRow, args: &[Expr]) -> RowAnalysis {
        let mut row = RowAnalysis::new(input.id, RowKind::Value);
        let mut problems = Problems::default();
        let result = self.stat_row_inner(input, r, args, &mut row, &mut problems);
        row.missing = slider_names(&problems.missing);
        if let Err(e) = result {
            row.error = Some(e);
        } else if let Some(e) = problems.first_message() {
            row.error = Some(e);
        }
        if row.error.is_some() {
            row.polygons.clear();
            row.points.clear();
            row.paths.clear();
            if let Some(s) = &mut row.stats {
                s.table = None;
            }
        }
        row
    }

    fn stat_row_inner(
        &mut self,
        input: &RowInput,
        r: StatRow,
        args: &[Expr],
        row: &mut RowAnalysis,
        problems: &mut Problems,
    ) -> Result<(), String> {
        let opts = &input.stats;
        let name = r.name();
        let usage = |shape: &str| format!("Try {name}({shape})");
        macro_rules! list {
            ($e:expr) => {
                match self.data($e, problems)? {
                    Some(v) => v,
                    None => return Ok(()),
                }
            };
        }
        macro_rules! number {
            ($e:expr, $default:expr, $what:expr) => {
                match self.number_arg($e, $default, $what, problems)? {
                    Some(v) => v,
                    None => return Ok(()),
                }
            };
        }
        let table_kind = |row: &mut RowAnalysis, table: stats::Table| {
            let mut out = StatsOut::new(StatKind::Table);
            out.table = Some(table);
            row.stats = Some(out);
        };
        match r {
            StatRow::Stats => {
                let [l] = args else {
                    return Err(usage("L"));
                };
                let v = list!(l);
                if v.is_empty() {
                    return Err("stats(L) needs at least one value".into());
                }
                table_kind(
                    row,
                    stats::Table {
                        rows: stats::summary(&v),
                        ..Default::default()
                    },
                );
            }
            StatRow::Histogram => {
                row.kind = RowKind::Curve;
                let (l, w) = match args {
                    [l] => (l, None),
                    [l, w] => (l, Some(w)),
                    _ => return Err(usage("L, w")),
                };
                let v = list!(l);
                let width = number!(w, Some(1.0), "The bin width");
                let bins = stats::bins(&v, width, opts.align)?;
                let total = v.iter().filter(|x| x.is_finite()).count();
                for (lo, count) in bins {
                    let h = stats::bar_height(count, total, width, opts.height);
                    row.polygons
                        .push(vec![[lo, 0.0], [lo + width, 0.0], [lo + width, h], [lo, h]]);
                }
                row.stats = Some(StatsOut::new(StatKind::Histogram));
            }
            StatRow::Dotplot => {
                row.kind = RowKind::Curve;
                let (l, w) = match args {
                    [l] => (l, None),
                    [l, w] => (l, Some(w)),
                    _ => return Err(usage("L")),
                };
                let v = list!(l);
                let width = match w {
                    None => None,
                    Some(w) => Some(number!(Some(w), None, "The bin width")),
                };
                row.points = stats::dots(&v, width, opts.align)?;
                row.stats = Some(StatsOut::new(StatKind::Dotplot));
            }
            StatRow::Boxplot => {
                row.kind = RowKind::Curve;
                let [l] = args else {
                    return Err(usage("L"));
                };
                let v = list!(l);
                let mut out = StatsOut::new(StatKind::Boxplot);
                let offset = self.setting(&opts.offset, 1.0, problems);
                let height = self.setting(&opts.box_height, 1.0, problems);
                let (offset, height) = match (offset, height) {
                    (Ok(o), Ok(h)) if o.is_finite() && h.is_finite() => (o, h),
                    (Err(e), _) | (_, Err(e)) => {
                        out.box_error = Some(e);
                        (1.0, 1.0)
                    }
                    _ => (1.0, 1.0),
                };
                row.stats = Some(out);
                let Some(b) = stats::box_plot(&v, !opts.whiskers_to_ends) else {
                    return Err("boxplot(L) needs at least one value".into());
                };
                let (lo, hi) = (offset - height / 2.0, offset + height / 2.0);
                for [a, c] in b.boxes {
                    row.polygons.push(vec![[a, lo], [c, lo], [c, hi], [a, hi]]);
                }
                row.paths
                    .push(vec![[b.whiskers[0], offset], [b.boxes[0][0], offset]]);
                row.paths
                    .push(vec![[b.boxes[1][1], offset], [b.whiskers[1], offset]]);
                row.points = b.outliers.iter().map(|x| [*x, offset]).collect();
            }
            StatRow::TTest => {
                let (l, mu) = match args {
                    [l] => (l, None),
                    [l, mu] => (l, Some(mu)),
                    _ => return Err(usage("L, μ_0")),
                };
                let v = list!(l);
                let mu = number!(mu, Some(0.0), "μ₀");
                table_kind(row, stats::ttest(&v, mu, opts.tail)?);
            }
            StatRow::ITTest => {
                let [a, b] = args else {
                    return Err(usage("L_1, L_2"));
                };
                let (a, b) = (list!(a), list!(b));
                table_kind(row, stats::ittest(&a, &b, opts.tail)?);
            }
            StatRow::ZTest => {
                let [l, mu, sigma] = args else {
                    return Err(usage("L, μ_0, σ"));
                };
                let v = list!(l);
                let mu = number!(Some(mu), None, "μ₀");
                let sigma = number!(Some(sigma), None, "σ");
                table_kind(row, stats::ztest(&v, mu, sigma, opts.tail)?);
            }
            StatRow::PropTest => {
                let [x, n, p] = args else {
                    return Err(usage("x, n, p_0") + ": x successes in n trials");
                };
                let x = number!(Some(x), None, "The successes");
                let n = number!(Some(n), None, "The trials");
                let p = number!(Some(p), None, "p₀");
                table_kind(row, stats::proptest(x, n, p, opts.tail)?);
            }
            StatRow::TwoPropTest => {
                let [x1, n1, x2, n2] = args else {
                    return Err(usage("x_1, n_1, x_2, n_2"));
                };
                let x1 = number!(Some(x1), None, "x₁");
                let n1 = number!(Some(n1), None, "n₁");
                let x2 = number!(Some(x2), None, "x₂");
                let n2 = number!(Some(n2), None, "n₂");
                table_kind(row, stats::twoproptest(x1, n1, x2, n2, opts.tail)?);
            }
            StatRow::ChisqGof => {
                let [o, e] = args else {
                    return Err(usage("O, E") + ": observed, then expected counts");
                };
                let (o, e) = (list!(o), list!(e));
                table_kind(row, stats::chisq_gof(&o, &e)?);
            }
            StatRow::ChisqTest => {
                if args.len() < 2 {
                    return Err(usage("R_1, R_2, …") + ": one list of counts per row");
                }
                let mut table = Vec::new();
                for a in args {
                    table.push(list!(a));
                }
                table_kind(row, stats::chisq_test(&table)?);
            }
            StatRow::TInterval => {
                let (l, c) = match args {
                    [l] => (l, None),
                    [l, c] => (l, Some(c)),
                    _ => return Err(usage("L, C")),
                };
                let v = list!(l);
                let c = number!(c, Some(0.95), "The confidence level");
                table_kind(row, stats::tinterval(&v, c)?);
            }
            StatRow::ITTInterval => {
                let (a, b, c) = match args {
                    [a, b] => (a, b, None),
                    [a, b, c] => (a, b, Some(c)),
                    _ => return Err(usage("L_1, L_2, C")),
                };
                let (a, b) = (list!(a), list!(b));
                let c = number!(c, Some(0.95), "The confidence level");
                table_kind(row, stats::ittinterval(&a, &b, c)?);
            }
            StatRow::ZInterval => {
                let (l, sigma, c) = match args {
                    [l, s] => (l, s, None),
                    [l, s, c] => (l, s, Some(c)),
                    _ => return Err(usage("L, σ, C")),
                };
                let v = list!(l);
                let sigma = number!(Some(sigma), None, "σ");
                let c = number!(c, Some(0.95), "The confidence level");
                table_kind(row, stats::zinterval(&v, sigma, c)?);
            }
            StatRow::PropInterval => {
                let (x, n, c) = match args {
                    [x, n] => (x, n, None),
                    [x, n, c] => (x, n, Some(c)),
                    _ => return Err(usage("x, n, C") + ": x successes in n trials"),
                };
                let x = number!(Some(x), None, "The successes");
                let n = number!(Some(n), None, "The trials");
                let c = number!(c, Some(0.95), "The confidence level");
                table_kind(row, stats::propinterval(x, n, c)?);
            }
            StatRow::TwoPropInterval => {
                let (x1, n1, x2, n2, c) = match args {
                    [a, b, d, e] => (a, b, d, e, None),
                    [a, b, d, e, c] => (a, b, d, e, Some(c)),
                    _ => return Err(usage("x_1, n_1, x_2, n_2, C")),
                };
                let x1 = number!(Some(x1), None, "x₁");
                let n1 = number!(Some(n1), None, "n₁");
                let x2 = number!(Some(x2), None, "x₂");
                let n2 = number!(Some(n2), None, "n₂");
                let c = number!(c, Some(0.95), "The confidence level");
                table_kind(row, stats::twopropinterval(x1, n1, x2, n2, c)?);
            }
        }
        Ok(())
    }

    /// A row whose value is a distribution draws it, as Desmos does: the
    /// density as a curve, or bars at the whole numbers for a discrete
    /// one, with the cumulative probability shaded when asked for.
    pub(super) fn dist_row(&mut self, input: &RowInput, d: Dist, row: &mut RowAnalysis) {
        row.value = None;
        let discrete = d.kind.discrete();
        let mut out = StatsOut::new(StatKind::Distribution { discrete });
        let (lo_end, hi_end) = d.support();
        if discrete {
            let lo = d.quantile(1e-9).max(lo_end).floor();
            let hi = d.quantile(1.0 - 1e-9).min(hi_end).ceil();
            if hi - lo <= MAX_BARS {
                let mut k = lo;
                while k <= hi {
                    let p = d.pdf(k);
                    if p > 0.0 {
                        row.polygons.push(bar(k, p));
                    }
                    k += 1.0;
                }
            }
        } else {
            let mut args: Vec<Low> = d.p[..d.kind.params()]
                .iter()
                .map(|v| Low::Const(*v))
                .collect();
            args.push(Low::Arg(Arg::X));
            let low = Low::Call(Builtin::Dist(d.kind, DistOp::Pdf), args);
            match Program::compile(&low) {
                Ok(program) => row.curves.push(Curve {
                    axis: Axis::X,
                    program,
                }),
                Err(e) => row.error = Some(e),
            }
        }
        if let Some([min, max]) = &input.stats.cdf {
            let mut problems = Problems::default();
            let lo = self.setting(min, f64::NEG_INFINITY, &mut problems);
            let hi = self.setting(max, f64::INFINITY, &mut problems);
            row.missing.extend(slider_names(&problems.missing));
            match (lo, hi) {
                (Ok(lo), Ok(hi)) if !lo.is_nan() && !hi.is_nan() => {
                    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
                    out.probability = Some(d.between(lo, hi));
                    if discrete {
                        let mut k = lo.ceil().max(lo_end);
                        let end = hi.floor().min(hi_end);
                        if end - k <= MAX_BARS {
                            while k <= end {
                                let p = d.pdf(k);
                                if p > 0.0 {
                                    row.polygons.push(bar(k, p));
                                }
                                k += 1.0;
                            }
                        }
                    } else if let Some(shade) = shade_under(&d, lo, hi) {
                        row.polygons.push(shade);
                    }
                }
                (Err(e), _) | (_, Err(e)) => out.cdf_error = Some(e),
                _ => {}
            }
        }
        row.stats = Some(out);
    }
}

/// A discrete distribution's bar at `k`, one unit wide.
fn bar(k: f64, p: f64) -> Vec<[f64; 2]> {
    vec![[k - 0.5, 0.0], [k + 0.5, 0.0], [k + 0.5, p], [k - 0.5, p]]
}

/// The region under a density from `lo` to `hi`, trimmed to where the
/// density is visibly above 0.
fn shade_under(d: &Dist, lo: f64, hi: f64) -> Option<Vec<[f64; 2]>> {
    let (lo_end, hi_end) = d.support();
    let lo = lo.max(d.quantile(1e-7)).max(lo_end);
    let hi = hi.min(d.quantile(1.0 - 1e-7)).min(hi_end);
    if !(lo < hi) || !lo.is_finite() || !hi.is_finite() {
        return None;
    }
    let mut outline: Vec<[f64; 2]> = (0..=SHADE_POINTS)
        .map(|i| {
            let x = lo + (hi - lo) * i as f64 / SHADE_POINTS as f64;
            let mut y = d.pdf(x);
            if !y.is_finite() {
                // Densities such as χ² with 1 degree of freedom run off to
                // ∞ at an end; stand just inside it.
                y = d.pdf(x + (hi - lo) * 1e-4 * if i == 0 { 1.0 } else { -1.0 });
            }
            [x, if y.is_finite() { y } else { 0.0 }]
        })
        .collect();
    outline.push([hi, 0.0]);
    outline.push([lo, 0.0]);
    Some(outline)
}
