//! Runs Solve on every row the golden tests graph, with every operation it
//! offers, to make sure none of them panics or runs past its time budget.

use graph_expr::symbolic::run;
use std::time::Instant;

#[test]
fn every_golden_row_and_operation_finishes() {
    let source = include_str!("golden.rs");
    let mut rows: Vec<String> = Vec::new();
    for part in source.split("r\"").skip(1) {
        if let Some(end) = part.find('"') {
            rows.push(part[..end].to_owned());
        }
    }
    for part in source.split("(\"").skip(1) {
        if let Some(end) = part.find('"') {
            rows.push(part[..end].replace("\\\\", "\\"));
        }
    }
    rows.extend(
        [
            "x^{x^{x}}=2",
            "\\tan x=\\sqrt{3}",
            "\\sec x=2",
            "\\cot x=0",
            "x^{10}-1=0",
            "\\left(x^2-1\\right)^{5}",
            "\\frac{1}{\\frac{1}{x}+1}",
            "\\int\\sin^{3}x dx",
            "\\int\\frac{1}{x^3+1}dx",
            "\\int x^{2}e^{3x}\\sin\\left(2x\\right)dx",
            "\\arcsin\\left(x\\right)=2",
            "e^{x}=-1",
            "\\sqrt{x}=-2",
            "\\ln x<1",
            "\\sin x>0",
            "0=0",
            "1=2",
            "x=x",
            "2^{x}=8",
            "\\log x=2",
            "\\left|2x+1\\right|\\le 3",
        ]
        .map(String::from),
    );
    assert!(rows.len() > 50, "only found {} rows", rows.len());
    for row in &rows {
        let first = run(row, &[], None);
        for c in &first.choices {
            let start = Instant::now();
            let _ = run(row, &[], Some(&c.op));
            assert!(
                start.elapsed().as_secs_f64() < 3.0,
                "{row} {} took too long",
                c.op
            );
        }
    }
}
