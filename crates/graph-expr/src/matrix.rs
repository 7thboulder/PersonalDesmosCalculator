//! Matrices: the numbers in a matrix row's grid, the matrix functions
//! (`det`, `inv`, `rref`, `eigenvalues`, ...) and the arithmetic between
//! them, and the pictures a 2×2 matrix draws on the graph.
//!
//! Matrices are worked out before the rest of the analysis, as a layer on
//! top of it: every row that mentions a matrix is rewritten so the matrix
//! parts become numbers, lists or points (`y = \det(A) x` becomes
//! `y = 3x`), and rows whose value is itself a matrix are finished here.
//! Rows without matrices are untouched.

use crate::analyze::Value;
use crate::ast::{Condition, Expr, Statement};
use std::collections::{HashMap, HashSet};

/// The functions that take or make matrices, by the name typed.
const FUNCTIONS: &[&str] = &[
    "det",
    "inv",
    "transpose",
    "rref",
    "trace",
    "rank",
    "eigenvalues",
    "eigenvectors",
    "identity",
];

/// Whether `name` is a matrix function, so `\det` and
/// `\operatorname{rref}` are read as calls.
pub fn is_function(name: &str) -> bool {
    FUNCTIONS.contains(&name)
}

/// Whether a row's LaTeX might use a matrix function or write a matrix
/// out, a quick test that spares graphs without matrices the layer.
pub(crate) fn may_mention(latex: &str) -> bool {
    FUNCTIONS.iter().any(|f| latex.contains(f)) || latex.contains("\\left[\\left[")
}

/// The most rows or columns a matrix can have.
const MAX_SIZE: usize = 20;

/// A matrix of real numbers, stored row by row.
#[derive(Clone, Debug, PartialEq)]
pub struct Matrix {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f64>,
}

impl Matrix {
    pub fn new(rows: usize, cols: usize, data: Vec<f64>) -> Matrix {
        debug_assert_eq!(data.len(), rows * cols);
        Matrix { rows, cols, data }
    }

    pub fn from_rows(rows: &[Vec<f64>]) -> Matrix {
        let cols = rows.first().map_or(0, Vec::len);
        Matrix::new(rows.len(), cols, rows.concat())
    }

    pub fn identity(n: usize) -> Matrix {
        let mut m = Matrix::new(n, n, vec![0.0; n * n]);
        for i in 0..n {
            m.set(i, i, 1.0);
        }
        m
    }

    pub fn get(&self, r: usize, c: usize) -> f64 {
        self.data[r * self.cols + c]
    }

    fn set(&mut self, r: usize, c: usize, v: f64) {
        self.data[r * self.cols + c] = v;
    }

    pub fn is_square(&self) -> bool {
        self.rows == self.cols
    }

    /// `rows×cols`, as errors name a size.
    pub fn size(&self) -> String {
        format!("{}×{}", self.rows, self.cols)
    }

    fn row(&self, r: usize) -> &[f64] {
        &self.data[r * self.cols..(r + 1) * self.cols]
    }

    pub fn transpose(&self) -> Matrix {
        let mut t = Matrix::new(self.cols, self.rows, vec![0.0; self.data.len()]);
        for r in 0..self.rows {
            for c in 0..self.cols {
                t.set(c, r, self.get(r, c));
            }
        }
        t
    }

    pub fn scale(&self, k: f64) -> Matrix {
        Matrix::new(
            self.rows,
            self.cols,
            self.data.iter().map(|v| v * k).collect(),
        )
    }

    pub fn add(&self, other: &Matrix, sign: f64) -> Result<Matrix, String> {
        if (self.rows, self.cols) != (other.rows, other.cols) {
            return Err(format!(
                "Can't add a {} matrix and a {} matrix; they need the same size",
                self.size(),
                other.size()
            ));
        }
        let data = self
            .data
            .iter()
            .zip(&other.data)
            .map(|(a, b)| a + sign * b)
            .collect();
        Ok(Matrix::new(self.rows, self.cols, data))
    }

    pub fn mul(&self, other: &Matrix) -> Result<Matrix, String> {
        if self.cols != other.rows {
            return Err(format!(
                "Can't multiply a {} matrix by a {} matrix; the first needs as many columns as the second has rows",
                self.size(),
                other.size()
            ));
        }
        let mut out = Matrix::new(self.rows, other.cols, vec![0.0; self.rows * other.cols]);
        for r in 0..self.rows {
            for c in 0..other.cols {
                let v = (0..self.cols)
                    .map(|k| self.get(r, k) * other.get(k, c))
                    .sum();
                out.set(r, c, v);
            }
        }
        Ok(out)
    }

    fn square(&self, what: &str) -> Result<(), String> {
        if self.is_square() {
            Ok(())
        } else {
            Err(format!(
                "Only square matrices have {what}; this one is {}",
                self.size()
            ))
        }
    }

    /// The largest entry's size, which tolerances are measured against.
    fn scale_of(&self) -> f64 {
        self.data
            .iter()
            .fold(0.0f64, |m, v| m.max(v.abs()))
            .max(1.0)
    }

    /// Rounds off what is left of cancelled entries, so `rref` shows 0
    /// rather than 1e-17, and -1 rather than -0.9999999999999993: each
    /// entry is kept to 13 significant figures.
    fn cleaned(mut self, scale: f64) -> Matrix {
        for v in &mut self.data {
            if v.abs() < 1e-12 * scale {
                *v = 0.0;
            } else {
                *v = format!("{v:.12e}").parse().unwrap_or(*v);
            }
        }
        self
    }

    pub fn det(&self) -> Result<f64, String> {
        self.square("a determinant")?;
        let n = self.rows;
        let mut a = self.clone();
        let mut det = 1.0;
        for k in 0..n {
            let p = (k..n)
                .max_by(|&i, &j| a.get(i, k).abs().total_cmp(&a.get(j, k).abs()))
                .expect("k < n");
            if a.get(p, k) == 0.0 {
                return Ok(0.0);
            }
            if p != k {
                a.swap_rows(p, k);
                det = -det;
            }
            let pivot = a.get(k, k);
            det *= pivot;
            for i in k + 1..n {
                let f = a.get(i, k) / pivot;
                for j in k..n {
                    let v = a.get(i, j) - f * a.get(k, j);
                    a.set(i, j, v);
                }
            }
        }
        // An integer matrix has an integer determinant; elimination leaves
        // it a hair off.
        if self.data.iter().all(|v| v.fract() == 0.0) && det.abs() < 1e15 {
            let rounded = det.round();
            if (det - rounded).abs() < 1e-9 * det.abs().max(1.0) {
                det = rounded;
            }
        }
        Ok(det)
    }

    fn swap_rows(&mut self, a: usize, b: usize) {
        for c in 0..self.cols {
            self.data.swap(a * self.cols + c, b * self.cols + c);
        }
    }

    /// Reduced row echelon form, with the columns that hold a pivot.
    /// Entries smaller than `tol` count as zero.
    fn rref_with(&self, tol: f64) -> (Matrix, Vec<usize>) {
        let mut a = self.clone();
        let mut pivots = Vec::new();
        let mut r = 0;
        for c in 0..a.cols {
            if r == a.rows {
                break;
            }
            let p = (r..a.rows)
                .max_by(|&i, &j| a.get(i, c).abs().total_cmp(&a.get(j, c).abs()))
                .expect("r < rows");
            if a.get(p, c).abs() <= tol {
                for i in r..a.rows {
                    a.set(i, c, 0.0);
                }
                continue;
            }
            a.swap_rows(p, r);
            let pivot = a.get(r, c);
            for j in 0..a.cols {
                let v = a.get(r, j) / pivot;
                a.set(r, j, v);
            }
            for i in 0..a.rows {
                if i == r {
                    continue;
                }
                let f = a.get(i, c);
                if f == 0.0 {
                    continue;
                }
                for j in 0..a.cols {
                    let v = a.get(i, j) - f * a.get(r, j);
                    a.set(i, j, v);
                }
            }
            pivots.push(c);
            r += 1;
        }
        (a, pivots)
    }

    fn tolerance(&self) -> f64 {
        1e-10 * self.scale_of() * self.rows.max(self.cols) as f64
    }

    pub fn rref(&self) -> Matrix {
        let scale = self.scale_of();
        self.rref_with(self.tolerance()).0.cleaned(scale)
    }

    pub fn rank(&self) -> usize {
        self.rref_with(self.tolerance()).1.len()
    }

    pub fn trace(&self) -> Result<f64, String> {
        self.square("a trace")?;
        Ok((0..self.rows).map(|i| self.get(i, i)).sum())
    }

    pub fn inverse(&self) -> Result<Matrix, String> {
        self.square("an inverse")?;
        let n = self.rows;
        let mut joined = Matrix::new(n, 2 * n, vec![0.0; 2 * n * n]);
        for r in 0..n {
            for c in 0..n {
                joined.set(r, c, self.get(r, c));
            }
            joined.set(r, n + r, 1.0);
        }
        let (reduced, pivots) = joined.rref_with(self.tolerance());
        if pivots.len() < n || pivots[n - 1] != n - 1 {
            return Err("This matrix has no inverse: its determinant is 0".into());
        }
        let mut inv = Matrix::new(n, n, vec![0.0; n * n]);
        for r in 0..n {
            for c in 0..n {
                inv.set(r, c, reduced.get(r, n + c));
            }
        }
        let scale = inv.scale_of();
        Ok(inv.cleaned(scale))
    }

    /// `A^k` for a whole number k; a negative k raises the inverse.
    pub fn power(&self, k: f64) -> Result<Matrix, String> {
        self.square("powers")?;
        if k.fract() != 0.0 || k.abs() > 1e6 {
            return Err("A matrix can only be raised to a whole number, like A^2 or A^{-1}".into());
        }
        let mut base = if k < 0.0 {
            self.inverse()?
        } else {
            self.clone()
        };
        let mut k = k.abs() as u64;
        let mut out = Matrix::identity(self.rows);
        while k > 0 {
            if k & 1 == 1 {
                out = out.mul(&base)?;
            }
            base = base.mul(&base)?;
            k >>= 1;
        }
        Ok(out)
    }

    /// Every eigenvalue, repeated as often as it is a root of the
    /// characteristic polynomial: real ones first from largest to
    /// smallest, then complex pairs.
    pub fn eigenvalues(&self) -> Result<Vec<(f64, f64)>, String> {
        self.square("eigenvalues")?;
        let scale = self.scale_of();
        let mut values = shifted_qr(hessenberg(self))
            .ok_or("These eigenvalues didn't settle; try a smaller matrix")?;
        // Imaginary parts that are only rounding noise, and repeated roots
        // that came out a hair apart, are tidied up.
        for v in &mut values {
            if v.1.abs() < 1e-9 * scale {
                v.1 = 0.0;
            }
            if v.0.abs() < 1e-12 * scale {
                v.0 = 0.0;
            }
        }
        values.sort_by(|a, b| {
            (a.1 != 0.0)
                .cmp(&(b.1 != 0.0))
                .then(b.0.total_cmp(&a.0))
                .then(b.1.total_cmp(&a.1))
        });
        Ok(values)
    }

    /// A basis of real eigenvectors, as columns, in the order of
    /// `eigenvalues`. Each is scaled so its last nonzero entry is 1 when
    /// that keeps it tidy, as worked by hand.
    pub fn eigenvectors(&self) -> Result<(Vec<f64>, Matrix), String> {
        let values = self.eigenvalues()?;
        let n = self.rows;
        let mut lambdas: Vec<f64> = Vec::new();
        let mut vectors: Vec<Vec<f64>> = Vec::new();
        let tol = 1e-6 * self.scale_of() * n as f64;
        for &(re, im) in &values {
            if im != 0.0 || lambdas.iter().any(|l| (l - re).abs() <= tol) {
                continue;
            }
            let shifted = self.add(&Matrix::identity(n).scale(re), -1.0)?;
            for v in null_space(&shifted, tol) {
                lambdas.push(re);
                vectors.push(v);
            }
        }
        if vectors.is_empty() {
            return Err("This matrix has no real eigenvectors; its eigenvalues are complex".into());
        }
        let mut m = Matrix::new(n, vectors.len(), vec![0.0; n * vectors.len()]);
        for (c, v) in vectors.iter().enumerate() {
            for (r, x) in v.iter().enumerate() {
                m.set(r, c, *x);
            }
        }
        Ok((lambdas, m))
    }
}

/// A basis of the vectors `m` sends to zero, read off its reduced form
/// with each free variable set to 1 in turn.
fn null_space(m: &Matrix, tol: f64) -> Vec<Vec<f64>> {
    let (reduced, pivots) = m.rref_with(tol);
    let free: Vec<usize> = (0..m.cols).filter(|c| !pivots.contains(c)).collect();
    free.iter()
        .map(|&f| {
            let mut v = vec![0.0; m.cols];
            v[f] = 1.0;
            for (r, &p) in pivots.iter().enumerate() {
                v[p] = -reduced.get(r, f);
            }
            for x in &mut v {
                if x.abs() < 1e-9 {
                    *x = 0.0;
                }
            }
            v
        })
        .collect()
}

/// A complex number, for the eigenvalue search.
#[derive(Clone, Copy, Debug, PartialEq)]
struct C(f64, f64);

impl C {
    fn abs(self) -> f64 {
        self.0.hypot(self.1)
    }
    fn conj(self) -> C {
        C(self.0, -self.1)
    }
    fn add(self, o: C) -> C {
        C(self.0 + o.0, self.1 + o.1)
    }
    fn sub(self, o: C) -> C {
        C(self.0 - o.0, self.1 - o.1)
    }
    fn mul(self, o: C) -> C {
        C(self.0 * o.0 - self.1 * o.1, self.0 * o.1 + self.1 * o.0)
    }
    fn scale(self, k: f64) -> C {
        C(self.0 * k, self.1 * k)
    }
    fn sqrt(self) -> C {
        let r = self.abs();
        let re = ((r + self.0) / 2.0).max(0.0).sqrt();
        let im = ((r - self.0) / 2.0).max(0.0).sqrt();
        C(re, if self.1 < 0.0 { -im } else { im })
    }
}

/// The same matrix in upper Hessenberg form (zero below the first
/// subdiagonal), by Householder reflections, which keep the eigenvalues.
#[allow(clippy::needless_range_loop)]
fn hessenberg(m: &Matrix) -> Vec<Vec<C>> {
    let n = m.rows;
    let mut a: Vec<Vec<f64>> = (0..n).map(|r| m.row(r).to_vec()).collect();
    for k in 0..n.saturating_sub(2) {
        let x: Vec<f64> = (k + 1..n).map(|i| a[i][k]).collect();
        let norm = x.iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm == 0.0 {
            continue;
        }
        let alpha = if x[0] > 0.0 { -norm } else { norm };
        let mut v = x;
        v[0] -= alpha;
        let vv: f64 = v.iter().map(|t| t * t).sum();
        if vv == 0.0 {
            continue;
        }
        for j in 0..n {
            let s: f64 = (0..v.len()).map(|i| v[i] * a[k + 1 + i][j]).sum();
            for i in 0..v.len() {
                a[k + 1 + i][j] -= 2.0 * s * v[i] / vv;
            }
        }
        for row in a.iter_mut() {
            let s: f64 = (0..v.len()).map(|j| row[k + 1 + j] * v[j]).sum();
            for j in 0..v.len() {
                row[k + 1 + j] -= 2.0 * s * v[j] / vv;
            }
        }
    }
    a.into_iter()
        .map(|row| row.into_iter().map(|v| C(v, 0.0)).collect())
        .collect()
}

/// The eigenvalues of a Hessenberg matrix by the QR algorithm with
/// Wilkinson shifts, in complex arithmetic so complex pairs need no
/// special case. None if it fails to converge.
#[allow(clippy::needless_range_loop)]
fn shifted_qr(mut h: Vec<Vec<C>>) -> Option<Vec<(f64, f64)>> {
    let n = h.len();
    let mut out = Vec::with_capacity(n);
    if n == 0 {
        return Some(out);
    }
    let mut hi = n - 1;
    let mut iterations = 0;
    loop {
        if hi == 0 {
            out.push((h[0][0].0, h[0][0].1));
            return Some(out);
        }
        // The lowest row of the block still being worked on.
        let mut lo = hi;
        while lo > 0 {
            let s = h[lo][lo].abs() + h[lo - 1][lo - 1].abs();
            let s = if s == 0.0 { 1.0 } else { s };
            if h[lo][lo - 1].abs() <= f64::EPSILON * s {
                h[lo][lo - 1] = C(0.0, 0.0);
                break;
            }
            lo -= 1;
        }
        if lo == hi {
            out.push((h[hi][hi].0, h[hi][hi].1));
            hi -= 1;
            iterations = 0;
            continue;
        }
        iterations += 1;
        if iterations > 200 {
            return None;
        }
        // The eigenvalue of the trailing 2×2 block nearer its corner.
        let (a, b, c, d) = (h[hi - 1][hi - 1], h[hi - 1][hi], h[hi][hi - 1], h[hi][hi]);
        let half = a.add(d).scale(0.5);
        let disc = half.mul(half).sub(a.mul(d).sub(b.mul(c))).sqrt();
        let (m1, m2) = (half.add(disc), half.sub(disc));
        let mut mu = if m1.sub(d).abs() < m2.sub(d).abs() {
            m1
        } else {
            m2
        };
        if iterations % 11 == 0 {
            // Now and then an unusual shift breaks a cycle.
            mu = d.add(C(c.abs(), 0.0));
        }
        for k in lo..=hi {
            h[k][k] = h[k][k].sub(mu);
        }
        let mut turns = Vec::with_capacity(hi - lo);
        for k in lo..hi {
            let (x, y) = (h[k][k], h[k + 1][k]);
            let r = x.abs().hypot(y.abs());
            let (cs, sn) = if r == 0.0 {
                (C(1.0, 0.0), C(0.0, 0.0))
            } else {
                (x.scale(1.0 / r), y.scale(1.0 / r))
            };
            for j in k..=hi {
                let (p, q) = (h[k][j], h[k + 1][j]);
                h[k][j] = cs.conj().mul(p).add(sn.conj().mul(q));
                h[k + 1][j] = cs.mul(q).sub(sn.mul(p));
            }
            turns.push((cs, sn));
        }
        for (k, (cs, sn)) in (lo..hi).zip(turns) {
            for row in h.iter_mut().take(hi + 1).skip(lo) {
                let (p, q) = (row[k], row[k + 1]);
                row[k] = p.mul(cs).add(q.mul(sn));
                row[k + 1] = q.mul(cs.conj()).sub(p.mul(sn.conj()));
            }
        }
        for k in lo..=hi {
            h[k][k] = h[k][k].add(mu);
        }
    }
}

// ---- Pictures on the graph -------------------------------------------------

/// What a stroke of a matrix's picture is, which sets its colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// The plane's grid lines, carried along by the matrix.
    Grid,
    /// Where (1, 0) goes.
    IHat,
    /// Where (0, 1) goes.
    JHat,
    /// The unit square's image, whose area is the determinant.
    Area,
    /// The lines eigenvectors span, which the matrix keeps.
    Eigen,
    /// The row's own colour.
    Row,
}

/// A line through the given points, or a filled shape with them as
/// corners.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub points: Vec<[f64; 2]>,
    pub ink: Ink,
    pub dashed: bool,
    pub filled: bool,
}

/// How far the grid reaches from the origin, in grid squares.
const GRID_REACH: i32 = 30;

fn apply(m: &[f64; 4], p: [f64; 2]) -> [f64; 2] {
    [m[0] * p[0] + m[1] * p[1], m[2] * p[0] + m[3] * p[1]]
}

/// An arrow from the origin to `tip`: its shaft and a filled head.
fn arrow(tip: [f64; 2], ink: Ink, out: &mut Vec<Stroke>) {
    let len = tip[0].hypot(tip[1]);
    if len == 0.0 || !len.is_finite() {
        return;
    }
    let (ux, uy) = (tip[0] / len, tip[1] / len);
    let head = (0.3f64).min(len * 0.4);
    let base = [tip[0] - ux * head, tip[1] - uy * head];
    let half = head * 0.45;
    out.push(Stroke {
        points: vec![[0.0, 0.0], base],
        ink,
        dashed: false,
        filled: false,
    });
    out.push(Stroke {
        points: vec![
            tip,
            [base[0] - uy * half, base[1] + ux * half],
            [base[0] + uy * half, base[1] - ux * half],
        ],
        ink,
        dashed: false,
        filled: true,
    });
}

/// The dashed line through the origin along `v`.
fn span(v: [f64; 2], out: &mut Vec<Stroke>) {
    let len = v[0].hypot(v[1]);
    if len == 0.0 || !len.is_finite() {
        return;
    }
    let (ux, uy) = (v[0] / len, v[1] / len);
    let reach = GRID_REACH as f64 * 4.0;
    // Short pieces, so the far ends stay accurate when drawn.
    let points = (-120..=120)
        .map(|i| {
            let t = reach * i as f64 / 120.0;
            [ux * t, uy * t]
        })
        .collect();
    out.push(Stroke {
        points,
        ink: Ink::Eigen,
        dashed: true,
        filled: false,
    });
}

/// The picture of a 2×2 matrix acting on the plane, `t` of the way from
/// doing nothing (0) to the whole matrix (1): the grid carried along,
/// the unit square's image, where (1, 0) and (0, 1) go, and the lines of
/// its real eigenvectors.
pub fn transformation(m: &Matrix, t: f64) -> Vec<Stroke> {
    let t = t.clamp(0.0, 1.0);
    let at = |r, c| {
        let i = if r == c { 1.0 } else { 0.0 };
        i + t * (m.get(r, c) - i)
    };
    let mt = [at(0, 0), at(0, 1), at(1, 0), at(1, 1)];
    let mut out = Vec::new();
    for k in -GRID_REACH..=GRID_REACH {
        let k = k as f64;
        // One point per grid square, so lines stay straight when drawn
        // far off screen.
        let line = |f: &dyn Fn(f64) -> [f64; 2]| -> Vec<[f64; 2]> {
            (-GRID_REACH..=GRID_REACH)
                .map(|s| apply(&mt, f(s as f64)))
                .collect()
        };
        for points in [line(&|s| [k, s]), line(&|s| [s, k])] {
            out.push(Stroke {
                points,
                ink: Ink::Grid,
                dashed: false,
                filled: false,
            });
        }
    }
    out.push(Stroke {
        points: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
            .iter()
            .map(|p| apply(&mt, *p))
            .collect(),
        ink: Ink::Area,
        dashed: false,
        filled: true,
    });
    if let Ok((_, vectors)) = m.eigenvectors() {
        for c in 0..vectors.cols {
            span([vectors.get(0, c), vectors.get(1, c)], &mut out);
        }
    }
    arrow(apply(&mt, [1.0, 0.0]), Ink::IHat, &mut out);
    arrow(apply(&mt, [0.0, 1.0]), Ink::JHat, &mut out);
    out
}

/// The picture of `eigenvectors(A)` for a 2×2 matrix: each eigenvector as
/// an arrow on its dashed line.
pub fn eigen_picture(vectors: &Matrix) -> Vec<Stroke> {
    let mut out = Vec::new();
    if vectors.rows != 2 {
        return out;
    }
    for c in 0..vectors.cols {
        let v = [vectors.get(0, c), vectors.get(1, c)];
        span(v, &mut out);
        arrow(v, Ink::Row, &mut out);
    }
    out
}

// ---- Reading rows -----------------------------------------------------------

/// What the layer needs from the rest of the analysis.
pub(crate) trait Scalars {
    /// The value of an expression without matrices in it.
    fn value(&mut self, e: &Expr) -> Result<Value, Failure>;
    /// Whether a variable is defined, so `A^T` knows T means transpose.
    fn defines(&self, name: &str) -> bool;
    /// Sets (or, with None, removes) what a variable means for the rest
    /// of the analysis, once its matrix parts are worked out.
    fn define(&mut self, name: &str, e: Option<Expr>);
    fn define_func(&mut self, name: &str, params: &[String], body: Expr);
}

/// Why a row could not be worked out, with any undefined names to offer
/// as sliders.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Failure {
    pub message: String,
    pub missing: Vec<String>,
}

impl From<String> for Failure {
    fn from(message: String) -> Failure {
        Failure {
            message,
            missing: Vec::new(),
        }
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Failure {
        Failure::from(message.to_owned())
    }
}

/// A matrix row: its name and the parsed cells of its grid.
pub(crate) struct Grid {
    pub name: String,
    pub cells: Vec<Vec<Option<Result<Expr, String>>>>,
}

/// A part of a row once its matrices are worked out.
enum Part {
    Matrix(Matrix),
    Expr(Expr),
}

/// What became of a row that mentions a matrix.
pub(crate) enum Outcome {
    /// It reads as ordinary math now.
    Rewritten(Statement),
    /// Its value is a matrix; `eigen` when it is `eigenvectors(...)`.
    Matrix {
        matrix: Matrix,
        eigen: bool,
    },
    Failed(Failure),
}

pub(crate) struct Layer {
    grids: HashMap<String, Grid>,
    /// Definitions that mention matrices, directly or through others.
    vars: HashMap<String, Expr>,
    funcs: HashMap<String, (Vec<String>, Expr)>,
    /// Every name whose meaning involves a matrix.
    names: HashSet<String>,
    /// Worked-out grids and variables: Some matrix, or None for a variable
    /// whose value is not a matrix (it is then defined for the rest).
    done: HashMap<String, Result<Option<Matrix>, Failure>>,
    funcs_done: HashMap<String, Result<(), Failure>>,
    /// The new definitions of variables and functions that are not
    /// matrices, with their matrix parts worked out.
    rewritten: HashMap<String, Expr>,
    visiting: HashSet<String>,
}

/// Whether `e` is `[[...], [...]]`, a matrix written out.
fn is_literal(e: &Expr) -> bool {
    matches!(e, Expr::List(items) if !items.is_empty() && items.iter().all(|i| matches!(i, Expr::List(_))))
}

impl Layer {
    pub fn new(
        grids: Vec<Grid>,
        vars: impl IntoIterator<Item = (String, Expr)>,
        funcs: impl IntoIterator<Item = (String, Vec<String>, Expr)>,
    ) -> Layer {
        let mut layer = Layer {
            names: grids.iter().map(|g| g.name.clone()).collect(),
            grids: grids.into_iter().map(|g| (g.name.clone(), g)).collect(),
            vars: HashMap::new(),
            funcs: HashMap::new(),
            done: HashMap::new(),
            funcs_done: HashMap::new(),
            rewritten: HashMap::new(),
            visiting: HashSet::new(),
        };
        let vars: Vec<(String, Expr)> = vars.into_iter().collect();
        let funcs: Vec<(String, Vec<String>, Expr)> = funcs.into_iter().collect();
        // Names that read matrices make the names that read them read
        // matrices too.
        loop {
            let mut grew = false;
            for (name, e) in &vars {
                if !layer.names.contains(name) && layer.mentions(e) {
                    layer.names.insert(name.clone());
                    layer.vars.insert(name.clone(), e.clone());
                    grew = true;
                }
            }
            for (name, params, body) in &funcs {
                if !layer.names.contains(name) && layer.mentions(body) {
                    layer.names.insert(name.clone());
                    layer
                        .funcs
                        .insert(name.clone(), (params.clone(), body.clone()));
                    grew = true;
                }
            }
            if !grew {
                return layer;
            }
        }
    }

    /// Whether any row involves a matrix, so the layer has work to do.
    pub fn active(&self) -> bool {
        !self.names.is_empty()
    }

    /// Whether `e` involves a matrix.
    pub fn mentions(&self, e: &Expr) -> bool {
        let mut found = false;
        e.visit_names(&mut |n, applied| {
            found |= self.names.contains(n) || (applied && is_function(n));
        });
        if !found {
            e.walk(&mut |e| found |= is_literal(e));
        }
        found
    }

    fn statement_mentions(&self, st: &Statement) -> bool {
        match st {
            Statement::Expr(e) | Statement::Ode(e) => self.mentions(e),
            Statement::Relation(a, _, b) | Statement::Regression(a, b) => {
                self.mentions(a) || self.mentions(b)
            }
            Statement::Chain(c) => c.operands.iter().any(|e| self.mentions(e)),
        }
    }

    /// The matrix in a grid row.
    pub fn grid(&mut self, name: &str, s: &mut dyn Scalars) -> Result<Matrix, Failure> {
        if let Some(done) = self.done.get(name) {
            return done.clone().map(|m| m.expect("grids are matrices"));
        }
        if !self.visiting.insert(name.to_owned()) {
            return Err(format!("'{name}' is defined in terms of itself").into());
        }
        let result = self.read_grid(name, s);
        self.visiting.remove(name);
        self.done.insert(name.to_owned(), result.clone().map(Some));
        result
    }

    fn read_grid(&mut self, name: &str, s: &mut dyn Scalars) -> Result<Matrix, Failure> {
        let cells: Vec<Vec<Option<Result<Expr, String>>>> = self.grids[name].cells.clone();
        let rows = cells.len();
        let cols = cells.first().map_or(0, Vec::len);
        if rows == 0 || cols == 0 || cells.iter().any(|r| r.len() != cols) {
            return Err("A matrix needs at least one row and one column".into());
        }
        let mut data = Vec::with_capacity(rows * cols);
        let mut missing: Vec<String> = Vec::new();
        let mut first: Option<String> = None;
        for cell in cells.iter().flatten() {
            let v = match cell {
                None => Ok(0.0),
                Some(Err(e)) => Err(Failure::from(e.clone())),
                Some(Ok(e)) => self.number(e, &[], s),
            };
            match v {
                Ok(v) => data.push(v),
                Err(f) => {
                    data.push(f64::NAN);
                    for m in f.missing {
                        if !missing.contains(&m) {
                            missing.push(m);
                        }
                    }
                    first.get_or_insert(f.message);
                }
            }
        }
        match first {
            Some(message) => Err(Failure { message, missing }),
            None => Ok(Matrix::new(rows, cols, data)),
        }
    }

    /// Works out a variable that mentions matrices.
    fn var(&mut self, name: &str, s: &mut dyn Scalars) -> Result<Option<Matrix>, Failure> {
        if self.grids.contains_key(name) {
            return self.grid(name, s).map(Some);
        }
        if let Some(done) = self.done.get(name) {
            return done.clone();
        }
        if !self.visiting.insert(name.to_owned()) {
            return Err(format!("'{name}' is defined in terms of itself").into());
        }
        let e = self.vars[name].clone();
        let result = match self.part(&e, &[], s) {
            Ok(Part::Matrix(m)) => {
                s.define(name, None);
                Ok(Some(m))
            }
            Ok(Part::Expr(e)) => {
                s.define(name, Some(e.clone()));
                self.rewritten.insert(name.to_owned(), e);
                Ok(None)
            }
            Err(f) => {
                s.define(name, None);
                Err(f)
            }
        };
        self.visiting.remove(name);
        self.done.insert(name.to_owned(), result.clone());
        result
    }

    /// Works out a function that mentions matrices, which must give
    /// ordinary values.
    fn func(&mut self, name: &str, s: &mut dyn Scalars) -> Result<(), Failure> {
        if let Some(done) = self.funcs_done.get(name) {
            return done.clone();
        }
        if !self.visiting.insert(name.to_owned()) {
            return Err(format!("'{name}' is defined in terms of itself").into());
        }
        let (params, body) = self.funcs[name].clone();
        let result = match self.part(&body, &params, s) {
            Ok(Part::Expr(e)) => {
                s.define_func(name, &params, e.clone());
                self.rewritten.insert(name.to_owned(), e);
                Ok(())
            }
            Ok(Part::Matrix(_)) => Err(
                "Functions can't give a matrix yet; define a matrix instead, like B = A^2".into(),
            ),
            Err(f) => Err(f),
        };
        self.visiting.remove(name);
        self.funcs_done.insert(name.to_owned(), result.clone());
        result
    }

    /// The new form of a row, or None when it has no matrices in it.
    pub fn row(&mut self, st: &Statement, s: &mut dyn Scalars) -> Option<Outcome> {
        if let Statement::Relation(lhs, crate::ast::Relation::Eq, _) = st {
            match lhs {
                Expr::Var(name) if self.vars.contains_key(name) => {
                    return Some(match self.var(name, s) {
                        Ok(Some(matrix)) => Outcome::Matrix {
                            eigen: matches!(&self.vars[name], Expr::Apply(f, _) if f == "eigenvectors"),
                            matrix,
                        },
                        Ok(None) => Outcome::Rewritten(Statement::Relation(
                            lhs.clone(),
                            crate::ast::Relation::Eq,
                            computed(self.rewritten[name].clone()),
                        )),
                        Err(f) => Outcome::Failed(f),
                    });
                }
                Expr::Apply(name, args) if self.funcs.contains_key(name) => {
                    return Some(match self.func(name, s) {
                        Ok(()) => Outcome::Rewritten(Statement::Relation(
                            Expr::Apply(name.clone(), args.clone()),
                            crate::ast::Relation::Eq,
                            self.rewritten[name].clone(),
                        )),
                        Err(f) => Outcome::Failed(f),
                    });
                }
                _ => {}
            }
        }
        if !self.statement_mentions(st) {
            return None;
        }
        Some(match self.statement(st, s) {
            Ok(o) => o,
            Err(f) => Outcome::Failed(f),
        })
    }

    fn statement(&mut self, st: &Statement, s: &mut dyn Scalars) -> Result<Outcome, Failure> {
        let mut side = |layer: &mut Layer, e: &Expr| -> Result<Expr, Failure> {
            match layer.part(e, &[], s)? {
                Part::Expr(e) => Ok(e),
                Part::Matrix(_) => Err(
                    "Matrices can't be compared or graphed; a matrix row shows its value".into(),
                ),
            }
        };
        Ok(Outcome::Rewritten(match st {
            Statement::Expr(e) => {
                return Ok(match self.part(e, &[], s)? {
                    Part::Matrix(matrix) => Outcome::Matrix {
                        eigen: matches!(e, Expr::Apply(f, _) if f == "eigenvectors"),
                        matrix,
                    },
                    Part::Expr(e) => Outcome::Rewritten(Statement::Expr(e)),
                })
            }
            Statement::Relation(a, r, b) => {
                let a = side(self, a)?;
                Statement::Relation(a, *r, side(self, b)?)
            }
            Statement::Regression(a, b) => {
                let a = side(self, a)?;
                Statement::Regression(a, side(self, b)?)
            }
            Statement::Ode(e) => Statement::Ode(side(self, e)?),
            Statement::Chain(c) => {
                let mut operands = Vec::with_capacity(c.operands.len());
                for e in &c.operands {
                    operands.push(side(self, e)?);
                }
                Statement::Chain(Condition {
                    operands,
                    relations: c.relations.clone(),
                })
            }
        }))
    }

    /// `e` as a single number, which matrix arithmetic needs.
    fn number(&mut self, e: &Expr, bound: &[String], s: &mut dyn Scalars) -> Result<f64, Failure> {
        match self.part(e, bound, s)? {
            Part::Matrix(_) => Err("A matrix can't go here; this needs a number".into()),
            Part::Expr(e) => match s.value(&e)? {
                Value::Number(v) => Ok(v),
                Value::Complex(v, 0.0) => Ok(v),
                _ => Err("This needs a number".into()),
            },
        }
    }

    fn expr(&mut self, e: &Expr, bound: &[String], s: &mut dyn Scalars) -> Result<Expr, Failure> {
        match self.part(e, bound, s)? {
            Part::Expr(e) => Ok(e),
            Part::Matrix(_) => Err("A matrix can't go here".into()),
        }
    }

    fn matrix(&mut self, e: &Expr, what: &str, s: &mut dyn Scalars) -> Result<Matrix, Failure> {
        match self.part(e, &[], s)? {
            Part::Matrix(m) => Ok(m),
            Part::Expr(_) => Err(format!("'{what}' needs a matrix, like {what}(A)").into()),
        }
    }

    /// Works out the matrix parts of `e`. Names in `bound` are a
    /// function's parameters or a sum's index, never matrices.
    fn part(&mut self, e: &Expr, bound: &[String], s: &mut dyn Scalars) -> Result<Part, Failure> {
        if !self.mentions(e) {
            return Ok(Part::Expr(e.clone()));
        }
        let both = |layer: &mut Layer, a: &Expr, b: &Expr, s: &mut dyn Scalars| {
            let a = layer.part(a, bound, s)?;
            let b = layer.part(b, bound, s)?;
            Ok::<_, Failure>((a, b))
        };
        Ok(match e {
            Expr::Var(n) if bound.contains(n) => Part::Expr(e.clone()),
            Expr::Var(n) if self.grids.contains_key(n) || self.vars.contains_key(n) => {
                match self.var(n, s)? {
                    Some(m) => Part::Matrix(m),
                    None => Part::Expr(e.clone()),
                }
            }
            Expr::Apply(f, args) if is_function(f) => self.call(f, args, bound, s)?,
            Expr::Apply(n, args) if self.funcs.contains_key(n) && !bound.contains(n) => {
                self.func(n, s)?;
                let mut out = Vec::with_capacity(args.len());
                for a in args {
                    out.push(self.expr(a, bound, s)?);
                }
                Part::Expr(Expr::Apply(n.clone(), out))
            }
            // `A(B)` and `A(1, 2)` multiply.
            Expr::Apply(n, args)
                if (self.grids.contains_key(n) || self.vars.contains_key(n))
                    && !bound.contains(n) =>
            {
                let left = self.part(&Expr::Var(n.clone()), bound, s)?;
                let right = match args.as_slice() {
                    [a] => self.part(a, bound, s)?,
                    [a, b] => Part::Expr(Expr::Point(Box::new(a.clone()), Box::new(b.clone()))),
                    [a, b, c] => Part::Expr(Expr::Point3(
                        Box::new(a.clone()),
                        Box::new(b.clone()),
                        Box::new(c.clone()),
                    )),
                    _ => return Err("A matrix multiplies one thing at a time".into()),
                };
                let right = match right {
                    Part::Expr(e) => Part::Expr(self.expr(&e, bound, s)?),
                    m => m,
                };
                self.product(left, right, s)?
            }
            Expr::List(items) if is_literal(e) => {
                let mut rows = Vec::with_capacity(items.len());
                for item in items {
                    let Expr::List(cells) = item else {
                        unreachable!("checked")
                    };
                    let mut row = Vec::with_capacity(cells.len());
                    for c in cells {
                        row.push(self.number(c, bound, s)?);
                    }
                    rows.push(row);
                }
                let cols = rows[0].len();
                if cols == 0 || rows.iter().any(|r| r.len() != cols) {
                    return Err("Every row of a matrix needs the same number of entries".into());
                }
                if rows.len() > MAX_SIZE || cols > MAX_SIZE {
                    return Err(format!("Matrices can be at most {MAX_SIZE}×{MAX_SIZE}").into());
                }
                Part::Matrix(Matrix::from_rows(&rows))
            }
            Expr::Neg(a) => match self.part(a, bound, s)? {
                Part::Matrix(m) => Part::Matrix(m.scale(-1.0)),
                Part::Expr(a) => Part::Expr(Expr::Neg(Box::new(a))),
            },
            Expr::Add(a, b) | Expr::Sub(a, b) => {
                let sign = if matches!(e, Expr::Add(..)) {
                    1.0
                } else {
                    -1.0
                };
                match both(self, a, b, s)? {
                    (Part::Matrix(a), Part::Matrix(b)) => Part::Matrix(a.add(&b, sign)?),
                    (Part::Expr(a), Part::Expr(b)) => Part::Expr(if sign > 0.0 {
                        Expr::bin(Expr::Add, a, b)
                    } else {
                        Expr::bin(Expr::Sub, a, b)
                    }),
                    _ => return Err("A number and a matrix can't be added".into()),
                }
            }
            Expr::Mul(a, b) => {
                let (a, b) = both(self, a, b, s)?;
                self.product(a, b, s)?
            }
            Expr::Div(a, b) => match both(self, a, b, s)? {
                (Part::Matrix(m), Part::Expr(k)) => {
                    let k = self.number(&k, bound, s)?;
                    Part::Matrix(m.scale(1.0 / k))
                }
                (Part::Expr(a), Part::Expr(b)) => Part::Expr(Expr::bin(Expr::Div, a, b)),
                _ => {
                    return Err(
                        "Can't divide by a matrix; multiply by its inverse, like A inv(B)".into(),
                    )
                }
            },
            Expr::Pow(a, b) => match self.part(a, bound, s)? {
                Part::Matrix(m) => {
                    if matches!(&**b, Expr::Var(t) if t == "T" && !s.defines("T")) {
                        Part::Matrix(m.transpose())
                    } else {
                        let k = self.number(b, bound, s)?;
                        Part::Matrix(m.power(k)?)
                    }
                }
                Part::Expr(a) => {
                    let b = self.expr(b, bound, s)?;
                    Part::Expr(Expr::bin(Expr::Pow, a, b))
                }
            },
            Expr::Index(a, i) => match self.part(a, bound, s)? {
                Part::Matrix(m) => {
                    let i = self.expr(i, bound, s)?;
                    Part::Expr(index(&m, s.value(&i)?)?)
                }
                Part::Expr(a) => {
                    let i = self.expr(i, bound, s)?;
                    Part::Expr(Expr::Index(Box::new(a), Box::new(i)))
                }
            },
            _ => Part::Expr(self.children(e, bound, s)?),
        })
    }

    /// `a b`, where either may be a matrix.
    fn product(&mut self, a: Part, b: Part, s: &mut dyn Scalars) -> Result<Part, Failure> {
        Ok(match (a, b) {
            (Part::Matrix(a), Part::Matrix(b)) => Part::Matrix(a.mul(&b)?),
            (Part::Expr(a), Part::Expr(b)) => Part::Expr(Expr::bin(Expr::Mul, a, b)),
            (Part::Expr(k), Part::Matrix(m)) => match s.value(&k)? {
                Value::Number(k) => Part::Matrix(m.scale(k)),
                _ => return Err("Only a number can multiply a matrix from the left; write the matrix first, like A(1, 2)".into()),
            },
            (Part::Matrix(m), Part::Expr(v)) => {
                // A point written out keeps its own expressions, so
                // `A (\cos t, \sin t)` is still a curve.
                let coords: Option<Vec<Expr>> = match &v {
                    Expr::Point(x, y) => Some(vec![(**x).clone(), (**y).clone()]),
                    Expr::Point3(x, y, z) => Some(vec![(**x).clone(), (**y).clone(), (**z).clone()]),
                    _ => None,
                };
                let coords = match coords {
                    Some(c) => c,
                    None => match s.value(&v)? {
                        Value::Number(k) => return Ok(Part::Matrix(m.scale(k))),
                        Value::Point(x, y) => vec![Expr::Num(x), Expr::Num(y)],
                        Value::Point3(p) => p.iter().map(|v| Expr::Num(*v)).collect(),
                        _ => return Err("A matrix can multiply a number, a point or another matrix".into()),
                    },
                };
                if coords.len() != m.cols {
                    return Err(format!(
                        "A {} matrix needs a point with {} coordinates",
                        m.size(),
                        m.cols
                    )
                    .into());
                }
                let out: Vec<Expr> = (0..m.rows)
                    .map(|r| linear(&(0..m.cols).map(|c| m.get(r, c)).collect::<Vec<_>>(), &coords))
                    .collect();
                let mut out = out.into_iter();
                let mut next = || Box::new(out.next().expect("sized"));
                Part::Expr(match m.rows {
                    2 => Expr::Point(next(), next()),
                    3 => Expr::Point3(next(), next(), next()),
                    _ => return Err(format!("A {} matrix gives a point with {} coordinates, which can't be drawn", m.size(), m.rows).into()),
                })
            }
        })
    }

    fn call(
        &mut self,
        f: &str,
        args: &[Expr],
        bound: &[String],
        s: &mut dyn Scalars,
    ) -> Result<Part, Failure> {
        let [arg] = args else {
            return Err(format!("Function '{f}' takes 1 argument").into());
        };
        if f == "identity" {
            let n = self.number(arg, bound, s)?;
            if n.fract() != 0.0 || !(1.0..=MAX_SIZE as f64).contains(&n) {
                return Err(format!("identity needs a whole number from 1 to {MAX_SIZE}").into());
            }
            return Ok(Part::Matrix(Matrix::identity(n as usize)));
        }
        let m = self.matrix(arg, f, s)?;
        let number = |v: f64| Part::Expr(Expr::Num(v));
        Ok(match f {
            "det" => number(m.det()?),
            "inv" => Part::Matrix(m.inverse()?),
            "transpose" => Part::Matrix(m.transpose()),
            "rref" => Part::Matrix(m.rref()),
            "trace" => number(m.trace()?),
            "rank" => number(m.rank() as f64),
            "eigenvalues" => Part::Expr(Expr::List(
                m.eigenvalues()?
                    .into_iter()
                    .map(|(re, im)| complex(re, im))
                    .collect(),
            )),
            "eigenvectors" => {
                let vectors = m.eigenvectors()?.1;
                let scale = vectors.scale_of();
                Part::Matrix(vectors.cleaned(scale))
            }
            _ => unreachable!("every matrix function is handled"),
        })
    }

    /// `e` with the matrix parts of its children worked out, for anything
    /// matrices only pass through, such as `\sqrt{\det A}`.
    fn children(
        &mut self,
        e: &Expr,
        bound: &[String],
        s: &mut dyn Scalars,
    ) -> Result<Expr, Failure> {
        let mut go = |layer: &mut Layer, a: &Expr| layer.expr(a, bound, s).map(Box::new);
        Ok(match e {
            Expr::Factorial(a) => Expr::Factorial(go(self, a)?),
            Expr::Point(a, b) => {
                let a = go(self, a)?;
                Expr::Point(a, go(self, b)?)
            }
            Expr::Point3(a, b, c) => {
                let a = go(self, a)?;
                let b = go(self, b)?;
                Expr::Point3(a, b, go(self, c)?)
            }
            Expr::Coord(a, c) => Expr::Coord(go(self, a)?, *c),
            Expr::List(items) => {
                let mut out = Vec::with_capacity(items.len());
                for i in items {
                    out.push(*go(self, i)?);
                }
                Expr::List(out)
            }
            Expr::Call(_, args) | Expr::Shape(_, args) => {
                let mut out = Vec::with_capacity(args.len());
                for a in args {
                    out.push(*go(self, a)?);
                }
                match e {
                    Expr::Call(f, _) => Expr::Call(*f, out),
                    Expr::Shape(shape, _) => Expr::Shape(*shape, out),
                    _ => unreachable!(),
                }
            }
            Expr::Apply(n, args) => {
                let mut out = Vec::with_capacity(args.len());
                for a in args {
                    out.push(*go(self, a)?);
                }
                Expr::Apply(n.clone(), out)
            }
            Expr::Piecewise(branches, otherwise) => {
                let mut out = Vec::with_capacity(branches.len());
                for (c, v) in branches {
                    let mut operands = Vec::with_capacity(c.operands.len());
                    for o in &c.operands {
                        operands.push(*go(self, o)?);
                    }
                    let condition = Condition {
                        operands,
                        relations: c.relations.clone(),
                    };
                    out.push((condition, *go(self, v)?));
                }
                let otherwise = match otherwise {
                    Some(o) => Some(go(self, o)?),
                    None => None,
                };
                Expr::Piecewise(out, otherwise)
            }
            Expr::Update(n, v) => Expr::Update(n.clone(), go(self, v)?),
            Expr::Actions(items) => {
                let mut out = Vec::with_capacity(items.len());
                for i in items {
                    out.push(*go(self, i)?);
                }
                Expr::Actions(out)
            }
            Expr::Range(a, b, c) => {
                let a = go(self, a)?;
                let b = match b {
                    Some(b) => Some(go(self, b)?),
                    None => None,
                };
                Expr::Range(a, b, go(self, c)?)
            }
            Expr::Filter(a, b) => {
                let a = go(self, a)?;
                Expr::Filter(a, go(self, b)?)
            }
            Expr::Repeat(..)
            | Expr::Integral(..)
            | Expr::Derivative(..)
            | Expr::Comprehension(..) => {
                // The variable these run over is never a matrix.
                let (var, inner): (Vec<String>, Vec<&Expr>) = match e {
                    Expr::Repeat(_, v, lo, hi, body) | Expr::Integral(v, lo, hi, body) => {
                        (vec![v.clone()], vec![lo, hi, body])
                    }
                    Expr::Derivative(v, body) => (vec![v.clone()], vec![body]),
                    Expr::Comprehension(body, gens) => (
                        gens.iter().map(|(n, _)| n.clone()).collect(),
                        std::iter::once(&**body)
                            .chain(gens.iter().map(|(_, l)| l))
                            .collect(),
                    ),
                    _ => unreachable!(),
                };
                let mut inner_bound = bound.to_vec();
                inner_bound.extend(var);
                let mut done = Vec::with_capacity(inner.len());
                for x in inner {
                    done.push(self.expr(x, &inner_bound, s)?);
                }
                let mut done = done.into_iter();
                let mut next = || Box::new(done.next().expect("sized"));
                match e {
                    Expr::Repeat(kind, v, ..) => {
                        Expr::Repeat(*kind, v.clone(), next(), next(), next())
                    }
                    Expr::Integral(v, ..) => Expr::Integral(v.clone(), next(), next(), next()),
                    Expr::Derivative(v, _) => Expr::Derivative(v.clone(), next()),
                    Expr::Comprehension(_, gens) => {
                        let body = next();
                        let gens = gens.iter().map(|(n, _)| (n.clone(), *next())).collect();
                        Expr::Comprehension(body, gens)
                    }
                    _ => unreachable!(),
                }
            }
            _ => return Err("Matrices can't be used here".into()),
        })
    }
}

/// A worked-out value that reads as a computation, so `d = \det(A)`
/// shows its value under the row rather than becoming a slider.
fn computed(e: Expr) -> Expr {
    let plain = |e: &Expr| e.as_literal().is_some();
    let literal = match &e {
        Expr::List(items) => items.iter().all(plain),
        Expr::Point(a, b) => plain(a) && plain(b),
        Expr::Point3(a, b, c) => plain(a) && plain(b) && plain(c),
        other => plain(other),
    };
    if literal {
        Expr::bin(Expr::Mul, Expr::Num(1.0), e)
    } else {
        e
    }
}

/// `re + im i` as an expression.
fn complex(re: f64, im: f64) -> Expr {
    if im == 0.0 {
        return Expr::Num(re);
    }
    let imag = Expr::bin(Expr::Mul, Expr::Num(im), Expr::Var("i".into()));
    if re == 0.0 {
        imag
    } else {
        Expr::bin(Expr::Add, Expr::Num(re), imag)
    }
}

/// `k_1 x_1 + k_2 x_2 + ...`, leaving out the zero terms.
fn linear(ks: &[f64], xs: &[Expr]) -> Expr {
    let mut sum: Option<Expr> = None;
    for (k, x) in ks.iter().zip(xs) {
        if *k == 0.0 {
            continue;
        }
        let term = if *k == 1.0 {
            x.clone()
        } else {
            Expr::bin(Expr::Mul, Expr::Num(*k), x.clone())
        };
        sum = Some(match sum {
            None => term,
            Some(s) => Expr::bin(Expr::Add, s, term),
        });
    }
    sum.unwrap_or(Expr::Num(0.0))
}

/// `A[i]`, the ith row as a list, or `A[i, j]`, one entry, counting from 1.
fn index(m: &Matrix, at: Value) -> Result<Expr, Failure> {
    let pick = |v: f64, n: usize, what: &str| -> Result<usize, Failure> {
        if v.fract() != 0.0 || v < 1.0 || v > n as f64 {
            return Err(
                format!("This matrix has no {what} {v}; it is {}×{}", m.rows, m.cols).into(),
            );
        }
        Ok(v as usize - 1)
    };
    match at {
        Value::Number(r) => {
            let r = pick(r, m.rows, "row")?;
            Ok(Expr::List(m.row(r).iter().map(|v| Expr::Num(*v)).collect()))
        }
        Value::Numbers(ij) if ij.len() == 2 => {
            let r = pick(ij[0], m.rows, "row")?;
            let c = pick(ij[1], m.cols, "column")?;
            Ok(Expr::Num(m.get(r, c)))
        }
        _ => Err("Pick an entry with A[row, column], or a row with A[row]".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(rows: &[&[f64]]) -> Matrix {
        Matrix::from_rows(&rows.iter().map(|r| r.to_vec()).collect::<Vec<_>>())
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn determinants_inverses_and_products() {
        let a = m(&[&[1.0, 2.0], &[3.0, 4.0]]);
        assert_eq!(a.det().unwrap(), -2.0);
        let inv = a.inverse().unwrap();
        assert_eq!(inv, m(&[&[-2.0, 1.0], &[1.5, -0.5]]));
        assert_eq!(a.mul(&inv).unwrap().cleaned(1.0), Matrix::identity(2));
        assert_eq!(a.power(-1.0).unwrap(), inv);
        assert_eq!(a.power(2.0).unwrap(), m(&[&[7.0, 10.0], &[15.0, 22.0]]));
        assert_eq!(a.transpose(), m(&[&[1.0, 3.0], &[2.0, 4.0]]));
        assert_eq!(a.trace().unwrap(), 5.0);
        let singular = m(&[&[1.0, 2.0], &[2.0, 4.0]]);
        assert_eq!(singular.det().unwrap(), 0.0);
        assert!(singular.inverse().is_err());
        assert_eq!(singular.rank(), 1);
        let three = m(&[&[2.0, 0.0, 1.0], &[1.0, 3.0, 2.0], &[1.0, 1.0, 2.0]]);
        assert_eq!(three.det().unwrap(), 6.0);
        assert!(m(&[&[1.0, 2.0, 3.0]]).det().is_err());
        assert!(a.mul(&m(&[&[1.0, 2.0, 3.0]])).is_err());
    }

    #[test]
    fn row_reduction() {
        let a = m(&[&[1.0, 2.0, 3.0], &[4.0, 5.0, 6.0], &[7.0, 8.0, 9.0]]);
        assert_eq!(
            a.rref(),
            m(&[&[1.0, 0.0, -1.0], &[0.0, 1.0, 2.0], &[0.0, 0.0, 0.0]])
        );
        assert_eq!(a.rank(), 2);
        let system = m(&[
            &[2.0, 1.0, -1.0, 8.0],
            &[-3.0, -1.0, 2.0, -11.0],
            &[-2.0, 1.0, 2.0, -3.0],
        ]);
        let r = system.rref();
        let last: Vec<f64> = (0..3).map(|i| r.get(i, 3)).collect();
        assert!(close(last[0], 2.0) && close(last[1], 3.0) && close(last[2], -1.0));
    }

    #[test]
    fn eigenvalues_real_complex_and_repeated() {
        let sym = m(&[&[2.0, 1.0], &[1.0, 2.0]]);
        let v = sym.eigenvalues().unwrap();
        assert!(close(v[0].0, 3.0) && close(v[1].0, 1.0), "{v:?}");
        let rotation = m(&[&[0.0, -1.0], &[1.0, 0.0]]);
        let v = rotation.eigenvalues().unwrap();
        assert!(close(v[0].0, 0.0) && close(v[0].1.abs(), 1.0), "{v:?}");
        assert!(rotation.eigenvectors().is_err());
        let shear = m(&[&[1.0, 1.0], &[0.0, 1.0]]);
        let v = shear.eigenvalues().unwrap();
        assert!(
            v.iter().all(|e| (e.0 - 1.0).abs() < 1e-6 && e.1 == 0.0),
            "{v:?}"
        );
        let (ls, vs) = shear.eigenvectors().unwrap();
        assert_eq!(ls.len(), 1);
        assert_eq!(vs, m(&[&[1.0], &[0.0]]));
        let big = m(&[
            &[4.0, 1.0, 0.0, 0.0],
            &[1.0, 3.0, 1.0, 0.0],
            &[0.0, 1.0, 2.0, 1.0],
            &[0.0, 0.0, 1.0, 1.0],
        ]);
        let v = big.eigenvalues().unwrap();
        let total: f64 = v.iter().map(|e| e.0).sum();
        assert!(close(total, 10.0), "{v:?}");
        let product: f64 = v.iter().map(|e| e.0).product();
        assert!((product - big.det().unwrap()).abs() < 1e-8, "{v:?}");
    }

    #[test]
    fn eigenvectors_are_tidy_and_correct() {
        let a = m(&[&[2.0, 1.0], &[1.0, 2.0]]);
        let (ls, vs) = a.eigenvectors().unwrap();
        assert!(close(ls[0], 3.0) && close(ls[1], 1.0));
        for (c, l) in ls.iter().enumerate() {
            let v = m(&[&[vs.get(0, c)], &[vs.get(1, c)]]);
            let av = a.mul(&v).unwrap();
            assert!(close(av.get(0, 0), l * v.get(0, 0)) && close(av.get(1, 0), l * v.get(1, 0)));
        }
        assert!(
            close(vs.get(0, 0), 1.0) && close(vs.get(1, 0), 1.0),
            "{vs:?}"
        );
        let diag = Matrix::identity(3).scale(2.0);
        let (ls, vs) = diag.eigenvectors().unwrap();
        assert_eq!(ls, vec![2.0, 2.0, 2.0]);
        assert_eq!(vs, Matrix::identity(3));
    }

    #[test]
    fn transformation_pictures() {
        let a = m(&[&[2.0, 1.0], &[1.0, 2.0]]);
        let start = transformation(&a, 0.0);
        let end = transformation(&a, 1.0);
        let tip =
            |p: &[Stroke], ink| p.iter().find(|s| s.ink == ink && s.filled).unwrap().points[0];
        assert_eq!(tip(&start, Ink::IHat), [1.0, 0.0]);
        assert_eq!(tip(&end, Ink::IHat), [2.0, 1.0]);
        assert_eq!(tip(&end, Ink::JHat), [1.0, 2.0]);
        let area = end.iter().find(|s| s.ink == Ink::Area).unwrap();
        assert_eq!(area.points[2], [3.0, 3.0]);
        assert_eq!(end.iter().filter(|s| s.ink == Ink::Eigen).count(), 2);
        assert_eq!(eigen_picture(&a.eigenvectors().unwrap().1).len(), 6);
    }
}
