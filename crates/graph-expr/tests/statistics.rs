//! Statistics rows, typed as MathQuill writes them.

use graph_expr::{analyze, RowAnalysis, RowInput, StatKind, StatOptions, Value};

fn row(latex: &str) -> RowInput {
    RowInput {
        latex: latex.into(),
        ..Default::default()
    }
}

fn with(latex: &str, stats: StatOptions) -> RowInput {
    RowInput {
        latex: latex.into(),
        stats,
        ..Default::default()
    }
}

fn run(mut input: Vec<RowInput>) -> Vec<RowAnalysis> {
    for (i, r) in input.iter_mut().enumerate() {
        r.id = i as u64 + 1;
    }
    analyze(&input)
}

fn number(r: &RowAnalysis) -> f64 {
    assert_eq!(r.error, None);
    match r.value {
        Some(Value::Number(v)) => v,
        ref other => panic!("not a number: {other:?}"),
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6 * (1.0 + b.abs())
}

const L: &str = "L=\\left[2,4,4,4,5,5,7,9\\right]";

#[test]
fn list_statistics() {
    let cases = [
        (
            "\\operatorname{stdev}\\left(L\\right)",
            (32.0f64 / 7.0).sqrt(),
        ),
        ("\\operatorname{stdevp}\\left(L\\right)", 2.0),
        ("\\operatorname{var}\\left(L\\right)", 32.0 / 7.0),
        ("\\operatorname{varp}\\left(L\\right)", 4.0),
        ("\\operatorname{mad}\\left(L\\right)", 1.5),
        ("\\operatorname{count}\\left(L\\right)", 8.0),
        ("\\operatorname{quartile}\\left(L,1\\right)", 4.0),
        ("\\operatorname{quartile}\\left(L,3\\right)", 6.0),
        ("\\operatorname{quantile}\\left(L,0.5\\right)", 4.5),
        ("\\operatorname{cov}\\left(L,L\\right)", 32.0 / 7.0),
        ("\\operatorname{corr}\\left(L,2L+1\\right)", 1.0),
        ("\\operatorname{spearman}\\left(L,L^{3}\\right)", 1.0),
        ("\\operatorname{tscore}\\left(L,5\\right)", 0.0),
    ];
    for (latex, want) in cases {
        let out = run(vec![row(L), row(latex)]);
        let got = number(&out[1]);
        assert!(close(got, want), "{latex}: {got} vs {want}");
    }
    // A list of quantiles.
    let out = run(vec![
        row(L),
        row("\\operatorname{quartile}\\left(L,\\left[0,4\\right]\\right)"),
    ]);
    assert_eq!(out[1].value, Some(Value::Numbers(vec![2.0, 9.0])));
    let out = run(vec![row(
        "\\operatorname{cov}\\left(\\left[1,2\\right],\\left[1,2,3\\right]\\right)",
    )]);
    assert!(out[0].error.as_deref().unwrap().contains("same length"));
}

#[test]
fn distributions() {
    let out = run(vec![
        row("D=\\operatorname{normaldist}\\left(\\right)"),
        row("D.\\operatorname{cdf}\\left(1.96\\right)"),
        row("D.\\operatorname{pdf}\\left(0\\right)"),
        row("D.\\operatorname{cdf}\\left(-1,1\\right)"),
        row("\\operatorname{quantile}\\left(D,0.975\\right)"),
        row("\\operatorname{mean}\\left(\\operatorname{binomialdist}\\left(10,0.3\\right)\\right)"),
        row("\\operatorname{stdev}\\left(\\operatorname{poissondist}\\left(4\\right)\\right)"),
        row("y=\\operatorname{tdist}\\left(5\\right).\\operatorname{pdf}\\left(x\\right)"),
    ]);
    assert!(close(number(&out[1]), 0.975_002_1));
    assert!(close(number(&out[2]), 0.398_942_28));
    assert!(close(number(&out[3]), 0.682_689_49));
    assert!((number(&out[4]) - 1.959_964).abs() < 1e-5);
    assert!(close(number(&out[5]), 3.0));
    assert!(close(number(&out[6]), 2.0));
    assert_eq!(out[7].error, None);
    assert_eq!(out[7].curves.len(), 1);
    // The distribution's own row draws its density, not a value.
    assert_eq!(out[0].error, None);
    assert_eq!(out[0].value, None);
    assert_eq!(out[0].curves.len(), 1);
    assert!(matches!(
        out[0].stats.as_ref().map(|s| s.kind),
        Some(StatKind::Distribution { discrete: false })
    ));
    let out = run(vec![row("\\operatorname{normaldist}\\left(0,-1\\right)")]);
    assert!(out[0].error.is_some());
    let out = run(vec![row("\\operatorname{normaldist}\\left(\\right)+1")]);
    assert!(out[0].error.as_deref().unwrap().contains("arithmetic"));
}

#[test]
fn cumulative_probability() {
    let opts = StatOptions {
        cdf: Some(["-1".into(), "1".into()]),
        ..Default::default()
    };
    let out = run(vec![with(
        "\\operatorname{normaldist}\\left(0,1\\right)",
        opts.clone(),
    )]);
    let s = out[0].stats.as_ref().unwrap();
    assert!(close(s.probability.unwrap(), 0.682_689_49));
    assert_eq!(out[0].polygons.len(), 1);
    // Discrete: bars, and both ends count.
    let opts = StatOptions {
        cdf: Some(["2".into(), "4".into()]),
        ..Default::default()
    };
    let out = run(vec![with(
        "\\operatorname{binomialdist}\\left(10,0.3\\right)",
        opts,
    )]);
    let s = out[0].stats.as_ref().unwrap();
    assert!((s.probability.unwrap() - 0.700_423).abs() < 1e-5);
    assert!(out[0].curves.is_empty());
    // 11 bars, then 3 more drawn over the ones in range.
    assert_eq!(out[0].polygons.len(), 14);
    // An empty bound is unbounded.
    let opts = StatOptions {
        cdf: Some(["".into(), "0".into()]),
        ..Default::default()
    };
    let out = run(vec![with(
        "\\operatorname{normaldist}\\left(\\right)",
        opts,
    )]);
    assert!(close(
        out[0].stats.as_ref().unwrap().probability.unwrap(),
        0.5
    ));
}

#[test]
fn random_draws() {
    let out = run(vec![
        row("R=\\operatorname{random}\\left(5\\right)"),
        row("\\operatorname{random}\\left(5\\right)"),
        row("\\operatorname{random}\\left(3,1\\right)"),
        row("\\operatorname{random}\\left(3,1\\right)"),
        row("\\operatorname{normaldist}\\left(10,2\\right).\\operatorname{random}\\left(2000,4\\right)"),
        row("\\operatorname{shuffle}\\left(\\left[1...5\\right]\\right)"),
        row("\\operatorname{random}\\left(\\left[7,8\\right]\\right)"),
    ]);
    let list = |r: &RowAnalysis| match &r.value {
        Some(Value::Numbers(v)) => v.clone(),
        other => panic!("{other:?} {:?}", r.error),
    };
    let a = list(&out[0]);
    assert_eq!(a.len(), 5);
    assert!(a.iter().all(|v| (0.0..1.0).contains(v)));
    // Different rows draw differently; a seed draws the same every time.
    assert_ne!(a, list(&out[1]));
    assert_eq!(list(&out[2]), list(&out[3]));
    let s = list(&out[4]);
    let m = s.iter().sum::<f64>() / s.len() as f64;
    assert!((m - 10.0).abs() < 0.2, "{m}");
    let mut sh = list(&out[5]);
    sh.sort_by(f64::total_cmp);
    assert_eq!(sh, vec![1.0, 2.0, 3.0, 4.0, 5.0]);
    let pick = number(&out[6]);
    assert!(pick == 7.0 || pick == 8.0);
    // Analyzing again draws the same numbers.
    let again = run(vec![row("R=\\operatorname{random}\\left(5\\right)")]);
    assert_eq!(list(&again[0]), a);
}

#[test]
fn plots() {
    let out = run(vec![
        row(L),
        row("\\operatorname{histogram}\\left(L\\right)"),
        row("\\operatorname{histogram}\\left(L,2\\right)"),
        row("\\operatorname{dotplot}\\left(L\\right)"),
        row("\\operatorname{boxplot}\\left(\\left[1,2,3,4,5,6,7,100\\right]\\right)"),
    ]);
    for r in &out[1..] {
        assert_eq!(r.error, None);
    }
    // Bins [2,3), [4,5), [5,6), [7,8), [9,10).
    assert_eq!(out[1].polygons.len(), 5);
    let tallest = out[1].polygons.iter().map(|p| p[2][1]).fold(0.0, f64::max);
    assert_eq!(tallest, 3.0);
    // Width 2: [2,4), [4,6), [6,8), [8,10).
    assert_eq!(out[2].polygons.len(), 4);
    assert_eq!(out[3].points.len(), 8);
    assert!(out[3].points.contains(&[4.0, 3.0]));
    // Two halves of the box, two whiskers, one outlier.
    assert_eq!(out[4].polygons.len(), 2);
    assert_eq!(out[4].paths.len(), 2);
    assert_eq!(out[4].points, vec![[100.0, 1.0]]);
    // Relative heights add to 1.
    let opts = StatOptions {
        height: graph_expr::stats::BarHeight::Relative,
        ..Default::default()
    };
    let out = run(vec![
        row(L),
        with("\\operatorname{histogram}\\left(L\\right)", opts),
    ]);
    let total: f64 = out[1].polygons.iter().map(|p| p[2][1]).sum();
    assert!(close(total, 1.0));
    // Frequencies.
    let out = run(vec![row(
        "\\operatorname{histogram}\\left(\\operatorname{freq}\\left(\\left[1,2\\right],\\left[3,5\\right]\\right)\\right)",
    )]);
    assert_eq!(out[0].error, None);
    assert_eq!(out[0].polygons[1][2][1], 5.0);
}

#[test]
fn tables() {
    let out = run(vec![
        row(L),
        row("\\operatorname{stats}\\left(L\\right)"),
        row("\\operatorname{ttest}\\left(L,4\\right)"),
        row("\\operatorname{tinterval}\\left(L,0.95\\right)"),
        row("\\operatorname{proptest}\\left(60,100,0.5\\right)"),
        row("\\operatorname{chisqtest}\\left(\\left[10,20\\right],\\left[20,10\\right]\\right)"),
        row("\\operatorname{ttest}\\left(L,m\\right)"),
    ]);
    for r in &out[1..6] {
        assert_eq!(r.error, None);
        assert!(r.stats.as_ref().unwrap().table.is_some());
    }
    let summary = &out[1].stats.as_ref().unwrap().table.as_ref().unwrap().rows;
    assert_eq!(summary[0].1, 8.0);
    let test = out[2].stats.as_ref().unwrap().table.as_ref().unwrap();
    assert!(test.tails);
    assert_eq!(out[6].missing, vec!["m".to_string()]);
    // One-sided p is half the two-sided one here.
    let opts = StatOptions {
        tail: graph_expr::stats::Tail::Greater,
        ..Default::default()
    };
    let one = run(vec![
        row(L),
        with("\\operatorname{ttest}\\left(L,4\\right)", opts),
    ]);
    let p1 = one[1].stats.as_ref().unwrap().table.as_ref().unwrap().rows[1].1;
    let p2 = test.rows[1].1;
    assert!(close(p1 * 2.0, p2));
}
