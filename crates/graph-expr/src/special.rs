//! Special functions beyond the ones Desmos has: gamma and its relatives,
//! the error functions, the Riemann zeta function, Bessel and Airy
//! functions, the trigonometric and exponential integrals, Lambert W, the
//! polylogarithm and the complete elliptic integrals. All work on real
//! numbers, and gamma and zeta on complex numbers too.

use std::f64::consts::{FRAC_2_SQRT_PI, FRAC_PI_2, PI};

/// The Euler–Mascheroni constant γ.
pub const EULER: f64 = 0.577_215_664_901_532_9;

fn is_int(x: f64) -> bool {
    x.fract() == 0.0 && x.abs() < 1e15
}

// ---------------------------------------------------------------------------
// Gamma, digamma, beta

/// Γ(x), undefined at 0 and the negative integers.
pub fn gamma(x: f64) -> f64 {
    if x <= 0.0 && is_int(x) {
        return f64::NAN;
    }
    libm::tgamma(x)
}

/// ln|Γ(x)|.
pub fn ln_gamma(x: f64) -> f64 {
    if x <= 0.0 && is_int(x) {
        return f64::NAN;
    }
    libm::lgamma(x)
}

/// The digamma function ψ(x) = Γ'(x)/Γ(x).
pub fn digamma(x: f64) -> f64 {
    if x.is_nan() || (x <= 0.0 && is_int(x)) {
        return f64::NAN;
    }
    if x < 0.0 {
        // ψ(1 - x) - ψ(x) = π cot(πx)
        return digamma(1.0 - x) - PI / (PI * x).tan();
    }
    let mut x = x;
    let mut r = 0.0;
    while x < 10.0 {
        r -= 1.0 / x;
        x += 1.0;
    }
    let f = 1.0 / (x * x);
    r + x.ln()
        - 0.5 / x
        - f * (1.0 / 12.0
            - f * (1.0 / 120.0 - f * (1.0 / 252.0 - f * (1.0 / 240.0 - f * (1.0 / 132.0)))))
}

/// B(a, b) = Γ(a)Γ(b)/Γ(a+b).
pub fn beta(a: f64, b: f64) -> f64 {
    if a > 0.0 && b > 0.0 {
        return (ln_gamma(a) + ln_gamma(b) - ln_gamma(a + b)).exp();
    }
    let (ga, gb, gab) = (gamma(a), gamma(b), gamma(a + b));
    if gab.is_nan() && !ga.is_nan() && !gb.is_nan() {
        return 0.0;
    }
    ga * gb / gab
}

// ---------------------------------------------------------------------------
// Error functions

pub fn erf(x: f64) -> f64 {
    libm::erf(x)
}

pub fn erfc(x: f64) -> f64 {
    libm::erfc(x)
}

/// The imaginary error function erfi(x) = -i erf(ix).
pub fn erfi(x: f64) -> f64 {
    let ax = x.abs();
    if ax < 5.0 {
        // 2/√π Σ x^(2n+1) / (n! (2n+1)), every term positive.
        let x2 = x * x;
        let mut term = x;
        let mut sum = x;
        for n in 1..200 {
            term *= x2 / n as f64;
            let t = term / (2 * n + 1) as f64;
            sum += t;
            if t.abs() < 1e-17 * sum.abs() {
                break;
            }
        }
        return FRAC_2_SQRT_PI * sum;
    }
    // e^(x²)/(x√π) Σ (2k-1)!! / (2x²)^k, stopped at its smallest term.
    let x2 = x * x;
    let mut term = 1.0;
    let mut sum = 1.0;
    for k in 1..60 {
        let next = term * (2 * k - 1) as f64 / (2.0 * x2);
        if next > term {
            break;
        }
        term = next;
        sum += term;
        if term < 1e-17 {
            break;
        }
    }
    x2.exp() / (x * PI.sqrt()) * sum
}

// ---------------------------------------------------------------------------
// Complex numbers, for complex gamma and zeta

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct C {
    pub re: f64,
    pub im: f64,
}

impl C {
    pub const fn new(re: f64, im: f64) -> C {
        C { re, im }
    }
    fn real(re: f64) -> C {
        C { re, im: 0.0 }
    }
    fn add(self, o: C) -> C {
        C::new(self.re + o.re, self.im + o.im)
    }
    fn sub(self, o: C) -> C {
        C::new(self.re - o.re, self.im - o.im)
    }
    fn mul(self, o: C) -> C {
        C::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
    fn scale(self, k: f64) -> C {
        C::new(self.re * k, self.im * k)
    }
    fn div(self, o: C) -> C {
        // Smith's method, which doesn't overflow for large parts.
        if o.re.abs() >= o.im.abs() {
            let r = o.im / o.re;
            let d = o.re + o.im * r;
            C::new((self.re + self.im * r) / d, (self.im - self.re * r) / d)
        } else {
            let r = o.re / o.im;
            let d = o.re * r + o.im;
            C::new((self.re * r + self.im) / d, (self.im * r - self.re) / d)
        }
    }
    fn exp(self) -> C {
        let m = self.re.exp();
        C::new(m * self.im.cos(), m * self.im.sin())
    }
    fn ln(self) -> C {
        C::new(self.re.hypot(self.im).ln(), self.im.atan2(self.re))
    }
    fn sin(self) -> C {
        C::new(
            self.re.sin() * self.im.cosh(),
            self.re.cos() * self.im.sinh(),
        )
    }
    /// a^self for a real, positive base.
    fn real_pow(self, base: f64) -> C {
        self.scale(base.ln()).exp()
    }
    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }
}

const LANCZOS_G: f64 = 7.0;
const LANCZOS: [f64; 9] = [
    0.999_999_999_999_809_9,
    676.520_368_121_885_1,
    -1_259.139_216_722_402_8,
    771.323_428_777_653_1,
    -176.615_029_162_140_6,
    12.507_343_278_686_905,
    -0.138_571_095_265_720_12,
    9.984_369_578_019_572e-6,
    1.505_632_735_149_311_6e-7,
];

/// Γ(z) for complex z.
pub fn gamma_complex(z: C) -> C {
    if z.im == 0.0 {
        return C::real(gamma(z.re));
    }
    if z.re < 0.5 {
        // Γ(z) Γ(1 - z) = π / sin(πz)
        let s = z.scale(PI).sin();
        return C::real(PI).div(s.mul(gamma_complex(C::real(1.0).sub(z))));
    }
    let z = z.sub(C::real(1.0));
    let mut a = C::real(LANCZOS[0]);
    for (i, c) in LANCZOS.iter().enumerate().skip(1) {
        a = a.add(C::real(*c).div(z.add(C::real(i as f64))));
    }
    let t = z.add(C::real(LANCZOS_G + 0.5));
    // √(2π) t^(z + 1/2) e^(-t) a, worked out through logarithms.
    let log = t.ln().mul(z.add(C::real(0.5))).sub(t).add(a.ln());
    log.exp().scale((2.0 * PI).sqrt())
}

// ---------------------------------------------------------------------------
// Riemann zeta

/// The coefficients d_k of Borwein's algorithm for the eta function.
fn borwein(n: usize) -> Vec<f64> {
    let nf = n as f64;
    let mut d = Vec::with_capacity(n + 1);
    let mut term = 1.0;
    let mut sum = 1.0;
    d.push(sum);
    for i in 1..=n {
        let i = i as f64;
        term *= 4.0 * (nf + i - 1.0) * (nf - i + 1.0) / ((2.0 * i) * (2.0 * i - 1.0));
        sum += term;
        d.push(sum);
    }
    d
}

/// ζ(s) for real s.
pub fn zeta(s: f64) -> f64 {
    if s.is_nan() || s == 1.0 {
        return f64::NAN;
    }
    if s == f64::INFINITY {
        return 1.0;
    }
    if s == 0.0 {
        return -0.5;
    }
    if s < 0.0 && is_int(s) && (s as i64) % 2 == 0 {
        return 0.0; // the trivial zeros
    }
    if s < 0.5 {
        // ζ(s) = 2^s π^(s-1) sin(πs/2) Γ(1-s) ζ(1-s)
        let g = gamma(1.0 - s);
        if !g.is_finite() {
            return f64::NAN;
        }
        return 2f64.powf(s) * PI.powf(s - 1.0) * (PI * s / 2.0).sin() * g * zeta(1.0 - s);
    }
    if s > 60.0 {
        return 1.0 + 2f64.powf(-s) + 3f64.powf(-s);
    }
    let n = 40;
    let d = borwein(n);
    let mut eta = 0.0;
    for k in 0..n {
        let t = (d[k] - d[n]) / ((k + 1) as f64).powf(s);
        eta += if k % 2 == 0 { t } else { -t };
    }
    eta = -eta / d[n];
    eta / (1.0 - 2f64.powf(1.0 - s))
}

/// ζ(s) for complex s.
pub fn zeta_complex(s: C) -> C {
    if s.im == 0.0 {
        return C::real(zeta(s.re));
    }
    if s.re < 0.5 {
        let one_minus = C::real(1.0).sub(s);
        let factor = s
            .real_pow(2.0)
            .mul(s.sub(C::real(1.0)).real_pow(PI))
            .mul(s.scale(PI / 2.0).sin())
            .mul(gamma_complex(one_minus));
        return factor.mul(zeta_complex(one_minus));
    }
    // More terms as the imaginary part grows, to keep the error down.
    let n = (40.0 + 0.9 * s.im.abs()).min(380.0) as usize;
    let d = borwein(n);
    let mut eta = C::real(0.0);
    for k in 0..n {
        let t = C::real(-(s.re) * ((k + 1) as f64).ln())
            .add(C::new(0.0, -s.im * ((k + 1) as f64).ln()))
            .exp()
            .scale(d[k] - d[n]);
        eta = if k % 2 == 0 { eta.add(t) } else { eta.sub(t) };
    }
    eta = eta.scale(-1.0 / d[n]);
    let denom = C::real(1.0).sub(C::real(1.0).sub(s).real_pow(2.0));
    eta.div(denom)
}

// ---------------------------------------------------------------------------
// Quadrature, for the Bessel functions of any order

/// Abscissas and weights of 10-point Gauss–Legendre on [-1, 1].
const GL_X: [f64; 5] = [
    0.148_874_338_981_631_2,
    0.433_395_394_129_247_2,
    0.679_409_568_299_024_4,
    0.865_063_366_688_984_5,
    0.973_906_528_517_171_7,
];
const GL_W: [f64; 5] = [
    0.295_524_224_714_752_9,
    0.269_266_719_309_996_4,
    0.219_086_362_515_982_0,
    0.149_451_349_150_580_6,
    0.066_671_344_308_688_1,
];

/// ∫ f over [a, b], split into `panels` pieces.
fn integrate(f: impl Fn(f64) -> f64, a: f64, b: f64, panels: usize) -> f64 {
    let h = (b - a) / panels as f64;
    let mut sum = 0.0;
    for p in 0..panels {
        let mid = a + (p as f64 + 0.5) * h;
        let half = h / 2.0;
        for (x, w) in GL_X.iter().zip(&GL_W) {
            sum += w * (f(mid - half * x) + f(mid + half * x));
        }
    }
    sum * h / 2.0
}

/// How far an integral of e^(-g(t)) needs to go before g(t) passes 45.
fn tail_end(g: impl Fn(f64) -> f64) -> f64 {
    let mut t = 0.5;
    while g(t) < 45.0 && t < 60.0 {
        t *= 1.25;
    }
    t
}

fn panels_for(width: f64) -> usize {
    (width.abs() * 2.0).clamp(8.0, 2000.0) as usize
}

/// J_ν(x) for x > 0 and any real order, from Bessel's integral.
fn bessel_j_integral(nu: f64, x: f64) -> f64 {
    let first = integrate(|t| (nu * t - x * t.sin()).cos(), 0.0, PI, panels_for(x + nu) + 4) / PI;
    let s = (nu * PI).sin();
    if s == 0.0 {
        return first;
    }
    let end = tail_end(|t| x * t.sinh() + nu * t);
    let tail = integrate(|t| (-x * t.sinh() - nu * t).exp(), 0.0, end, 8 * end.ceil() as usize);
    first - s / PI * tail
}

/// The Bessel function of the first kind J_ν(x).
pub fn bessel_j(nu: f64, x: f64) -> f64 {
    if nu.is_nan() || x.is_nan() {
        return f64::NAN;
    }
    if is_int(nu) && nu.abs() < 1e6 {
        return libm::jn(nu as i32, x);
    }
    if x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return if nu > 0.0 { 0.0 } else { f64::NAN };
    }
    bessel_j_integral(nu, x)
}

/// The Bessel function of the second kind Y_ν(x).
pub fn bessel_y(nu: f64, x: f64) -> f64 {
    if nu.is_nan() || x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if is_int(nu) && nu.abs() < 1e6 {
        return libm::yn(nu as i32, x);
    }
    let (s, c) = (nu * PI).sin_cos();
    (bessel_j_integral(nu, x) * c - bessel_j_integral(-nu, x)) / s
}

/// The modified Bessel function of the first kind I_ν(x).
pub fn bessel_i(nu: f64, x: f64) -> f64 {
    if nu.is_nan() || x.is_nan() {
        return f64::NAN;
    }
    let integer = is_int(nu);
    if x < 0.0 {
        if !integer {
            return f64::NAN;
        }
        let v = bessel_i(nu, -x);
        return if (nu as i64) % 2 == 0 { v } else { -v };
    }
    if x == 0.0 {
        return if nu == 0.0 {
            1.0
        } else if nu > 0.0 || integer {
            0.0
        } else {
            f64::NAN
        };
    }
    if x > 700.0 {
        return f64::INFINITY;
    }
    // Scaled by e^(-x) inside, so large x doesn't overflow early.
    let first = integrate(
        |t| (x * (t.cos() - 1.0)).exp() * (nu * t).cos(),
        0.0,
        PI,
        (16.0 + x / 4.0 + nu.abs()).min(600.0) as usize,
    ) / PI
        * x.exp();
    let s = (nu * PI).sin();
    if integer || s == 0.0 {
        return first;
    }
    let end = tail_end(|t| x * t.cosh() + nu * t);
    let tail = integrate(|t| (-x * t.cosh() - nu * t).exp(), 0.0, end, 8 * end.ceil() as usize);
    first - s / PI * tail
}

/// The modified Bessel function of the second kind K_ν(x).
pub fn bessel_k(nu: f64, x: f64) -> f64 {
    if nu.is_nan() || x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::INFINITY;
    }
    let nu = nu.abs();
    // ∫₀^∞ e^(-x cosh t) cosh(νt) dt, with e^(-x) taken out.
    let end = tail_end(|t| x * (t.cosh() - 1.0) - nu * t);
    integrate(
        |t| (-x * (t.cosh() - 1.0)).exp() * (nu * t).cosh(),
        0.0,
        end,
        (8.0 * end.ceil()).max(16.0) as usize,
    ) * (-x).exp()
}

// ---------------------------------------------------------------------------
// Airy functions

const AI0: f64 = 0.355_028_053_887_817_2;
const AIP0: f64 = 0.258_819_403_792_806_8; // -Ai'(0)

/// The Maclaurin series f and g, with Ai = Ai(0) f + Ai'(0) g.
fn airy_series(x: f64) -> (f64, f64) {
    let x3 = x * x * x;
    let (mut f, mut g) = (1.0, x);
    let (mut tf, mut tg) = (1.0, x);
    for k in 1..60 {
        let k = k as f64;
        tf *= x3 / ((3.0 * k) * (3.0 * k - 1.0));
        tg *= x3 / ((3.0 * k + 1.0) * (3.0 * k));
        f += tf;
        g += tg;
        if tf.abs() < 1e-18 && tg.abs() < 1e-18 {
            break;
        }
    }
    (f, g)
}

pub fn airy_ai(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x.abs() <= 2.0 {
        let (f, g) = airy_series(x);
        return AI0 * f - AIP0 * g;
    }
    if x > 0.0 {
        if x > 105.0 {
            return 0.0;
        }
        let zeta = 2.0 / 3.0 * x.powf(1.5);
        (x / 3.0).sqrt() / PI * bessel_k(1.0 / 3.0, zeta)
    } else {
        let y = -x;
        let zeta = 2.0 / 3.0 * y.powf(1.5);
        y.sqrt() / 3.0 * (bessel_j_integral(1.0 / 3.0, zeta) + bessel_j_integral(-1.0 / 3.0, zeta))
    }
}

pub fn airy_bi(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x.abs() <= 2.0 {
        let (f, g) = airy_series(x);
        return 3f64.sqrt() * (AI0 * f + AIP0 * g);
    }
    let zeta = 2.0 / 3.0 * x.abs().powf(1.5);
    if x > 0.0 {
        (x / 3.0).sqrt() * (bessel_i(1.0 / 3.0, zeta) + bessel_i(-1.0 / 3.0, zeta))
    } else {
        let y = -x;
        (y / 3.0).sqrt() * (bessel_j_integral(-1.0 / 3.0, zeta) - bessel_j_integral(1.0 / 3.0, zeta))
    }
}

// ---------------------------------------------------------------------------
// Trigonometric, exponential and logarithmic integrals

/// (Ci(x), Si(x)) for x > 0.
fn cisi(x: f64) -> (f64, f64) {
    if x > 2.0 {
        // A continued fraction for E₁(ix), as in Numerical Recipes.
        let tiny = 1e-300;
        let mut b = C::new(1.0, x);
        let mut c = C::real(1.0 / tiny);
        let mut d = C::real(1.0).div(b);
        let mut h = d;
        for i in 2..1000 {
            let a = -((i - 1) * (i - 1)) as f64;
            b = b.add(C::real(2.0));
            d = C::real(1.0).div(d.scale(a).add(b));
            c = b.add(C::real(a).div(c));
            let del = c.mul(d);
            h = h.mul(del);
            if (del.re - 1.0).abs() + del.im.abs() < 1e-16 {
                break;
            }
        }
        let h = C::new(x.cos(), -x.sin()).mul(h);
        return (-h.re, FRAC_PI_2 + h.im);
    }
    let x2 = x * x;
    let mut si = x;
    let mut ci = 0.0;
    let mut fact = x; // x^k / k!
    for k in 2..60 {
        fact *= x / k as f64;
        if k % 2 == 1 {
            let t = fact / k as f64;
            if (k / 2) % 2 == 1 {
                si -= t;
            } else {
                si += t;
            }
        } else {
            let t = fact / k as f64;
            if (k / 2) % 2 == 1 {
                ci -= t;
            } else {
                ci += t;
            }
        }
        if fact < 1e-18 * x2.max(1e-300) {
            break;
        }
    }
    (EULER + x.ln() + ci, si)
}

/// The sine integral Si(x).
pub fn si(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        return 0.0;
    }
    if x.is_infinite() {
        return FRAC_PI_2.copysign(x);
    }
    let s = cisi(x.abs()).1;
    s.copysign(x)
}

/// The cosine integral Ci(x), for x > 0.
pub fn ci(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x == f64::INFINITY {
        return 0.0;
    }
    cisi(x).0
}

/// E₁(x) for x > 0.
fn e1(x: f64) -> f64 {
    if x > 1.0 {
        let tiny = 1e-300;
        let mut b = x + 1.0;
        let mut c = 1.0 / tiny;
        let mut d = 1.0 / b;
        let mut h = d;
        for i in 1..1000 {
            let a = -((i * i) as f64);
            b += 2.0;
            d = 1.0 / (a * d + b);
            c = b + a / c;
            let del = c * d;
            h *= del;
            if (del - 1.0).abs() < 1e-16 {
                break;
            }
        }
        return h * (-x).exp();
    }
    let mut sum = 0.0;
    let mut term = 1.0;
    for k in 1..100 {
        term *= -x / k as f64;
        let t = term / k as f64;
        sum += t;
        if t.abs() < 1e-18 {
            break;
        }
    }
    -EULER - x.ln() - sum
}

/// The exponential integral Ei(x).
pub fn ei(x: f64) -> f64 {
    if x.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    if x < 0.0 {
        return -e1(-x);
    }
    if x > 710.0 {
        return f64::INFINITY;
    }
    if x > 40.0 {
        // e^x/x Σ k!/x^k, stopped at its smallest term.
        let mut term = 1.0;
        let mut sum = 1.0;
        for k in 1..60 {
            let next = term * k as f64 / x;
            if next > term {
                break;
            }
            term = next;
            sum += term;
            if term < 1e-17 {
                break;
            }
        }
        return x.exp() / x * sum;
    }
    let mut sum = 0.0;
    let mut term = 1.0;
    for k in 1..400 {
        term *= x / k as f64;
        let t = term / k as f64;
        sum += t;
        if t < 1e-17 * sum {
            break;
        }
    }
    EULER + x.ln() + sum
}

/// The logarithmic integral li(x) = Ei(ln x).
pub fn li(x: f64) -> f64 {
    if x.is_nan() || x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return 0.0;
    }
    if x == 1.0 {
        return f64::NEG_INFINITY;
    }
    ei(x.ln())
}

// ---------------------------------------------------------------------------
// Lambert W

/// The principal branch W₀(x), the w ≥ -1 with w e^w = x.
pub fn lambert_w(x: f64) -> f64 {
    let branch = -1.0 / std::f64::consts::E;
    if x.is_nan() || x < branch {
        return f64::NAN;
    }
    if x == branch {
        return -1.0;
    }
    if x == 0.0 {
        return 0.0;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }
    let mut w = if x < -0.3 {
        let p = (2.0 * (std::f64::consts::E * x + 1.0)).sqrt();
        -1.0 + p - p * p / 3.0 + 11.0 / 72.0 * p * p * p
    } else if x < 3.0 {
        (1.0 + x).ln() * 0.8
    } else {
        let l = x.ln();
        l - l.ln()
    };
    for _ in 0..100 {
        let ew = w.exp();
        let f = w * ew - x;
        let wp1 = w + 1.0;
        let denom = ew * wp1 - (w + 2.0) * f / (2.0 * wp1);
        if denom == 0.0 || !denom.is_finite() {
            break;
        }
        let next = w - f / denom;
        if (next - w).abs() <= 1e-15 * (1.0 + next.abs()) {
            w = next;
            break;
        }
        w = next;
    }
    w
}

// ---------------------------------------------------------------------------
// Polylogarithm

fn factorial(n: u32) -> f64 {
    (1..=n).fold(1.0, |a, k| a * k as f64)
}

/// Li_s(x) for real s and real x.
pub fn polylog(s: f64, x: f64) -> f64 {
    if s.is_nan() || x.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        return 0.0;
    }
    if x == 1.0 {
        return if s > 1.0 { zeta(s) } else { f64::INFINITY };
    }
    if x > 1.0 {
        return f64::NAN; // complex
    }
    if x.abs() <= 0.5 {
        let mut sum = 0.0;
        let mut p = 1.0;
        for k in 1..200 {
            p *= x;
            let t = p / (k as f64).powf(s);
            sum += t;
            if t.abs() < 1e-17 * sum.abs().max(1e-300) {
                break;
            }
        }
        return sum;
    }
    if x > 0.5 {
        return polylog_near_one(s, x.ln());
    }
    if x >= -1.0 {
        // Li_s(x) + Li_s(-x) = 2^(1-s) Li_s(x²)
        return 2f64.powf(1.0 - s) * polylog(s, x * x) - polylog(s, -x);
    }
    // x < -1: the inversion formula, for whole-number orders.
    if !is_int(s) || s.abs() > 100.0 {
        return f64::NAN;
    }
    let n = s as i64;
    let mu = (-x).ln();
    let mut sum = 0.0;
    if n >= 0 {
        for k in 0..=(n / 2) {
            let eta = if k == 0 {
                0.5
            } else {
                (1.0 - 2f64.powi(1 - 2 * k as i32)) * zeta(2.0 * k as f64)
            };
            sum += mu.powi((n - 2 * k) as i32) / factorial((n - 2 * k) as u32) * eta;
        }
    }
    let sign = if n % 2 == 0 { 1.0 } else { -1.0 };
    -sign * polylog(s, 1.0 / x) - 2.0 * sum
}

/// Li_s(e^μ) for small negative μ, from its expansion in powers of μ.
fn polylog_near_one(s: f64, mu: f64) -> f64 {
    let mut sum = 0.0;
    let mut p = 1.0; // μ^k / k!
    let positive_int = is_int(s) && s >= 1.0;
    for k in 0..60u32 {
        if k > 0 {
            p *= mu / k as f64;
        }
        if positive_int && k as f64 == s - 1.0 {
            continue;
        }
        let t = zeta(s - k as f64) * p;
        // ζ is zero at every other term, so the sum always runs its course.
        if t.is_finite() {
            sum += t;
        }
    }
    if positive_int {
        let n = s as u32;
        let harmonic: f64 = (1..n).map(|k| 1.0 / k as f64).sum();
        sum + mu.powi(n as i32 - 1) / factorial(n - 1) * (harmonic - (-mu).ln())
    } else {
        sum + gamma(1.0 - s) * (-mu).powf(s - 1.0)
    }
}

// ---------------------------------------------------------------------------
// Complete elliptic integrals, with modulus k

/// (K(k), E(k)) by the arithmetic–geometric mean.
fn elliptic(k: f64) -> (f64, f64) {
    if k.is_nan() || k.abs() > 1.0 {
        return (f64::NAN, f64::NAN);
    }
    if k.abs() == 1.0 {
        return (f64::INFINITY, 1.0);
    }
    let mut a = 1.0;
    let mut b = (1.0 - k * k).sqrt();
    let mut sum = k * k / 2.0;
    let mut power = 0.5;
    for _ in 0..40 {
        let c = (a - b) / 2.0;
        let (na, nb) = ((a + b) / 2.0, (a * b).sqrt());
        power *= 2.0;
        sum += power * c * c;
        a = na;
        b = nb;
        if c.abs() < 1e-17 {
            break;
        }
    }
    let kk = PI / (2.0 * a);
    (kk, kk * (1.0 - sum))
}

pub fn elliptic_k(k: f64) -> f64 {
    elliptic(k).0
}

pub fn elliptic_e(k: f64) -> f64 {
    elliptic(k).1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol * (1.0 + b.abs())
    }

    macro_rules! check {
        ($got:expr, $want:expr) => {
            check!($got, $want, 1e-11)
        };
        ($got:expr, $want:expr, $tol:expr) => {{
            let (g, w) = ($got, $want);
            assert!(close(g, w, $tol), "{} = {g}, want {w}", stringify!($got));
        }};
    }

    #[test]
    fn gamma_family() {
        check!(gamma(0.5), PI.sqrt());
        check!(gamma(5.0), 24.0);
        check!(gamma(-1.5), 2.363_271_801_207_355);
        assert!(gamma(-2.0).is_nan());
        check!(digamma(1.0), -EULER);
        check!(digamma(0.5), -EULER - 2.0 * 2f64.ln());
        check!(digamma(-0.5), 0.036_489_973_978_576_52);
        check!(beta(2.0, 3.0), 1.0 / 12.0);
        let g = gamma_complex(C::new(1.0, 1.0));
        check!(g.re, 0.498_015_668_118_356);
        check!(g.im, -0.154_949_828_301_810_7);
        let g = gamma_complex(C::new(-0.5, 2.0));
        check!(g.re, -0.039_038_849_162_115_52, 1e-9);
        check!(g.im, -0.035_167_876_062_686_94, 1e-9);
    }

    #[test]
    fn error_functions() {
        check!(erf(1.0), 0.842_700_792_949_714_9);
        check!(erfc(2.0), 0.004_677_734_981_047_266);
        check!(erfi(1.0), 1.650_425_758_797_543);
        check!(erfi(6.0), 4.112_751_455_828_239e14, 1e-9);
        check!(erfi(-0.3), -0.348_949_338_758_936_2);
    }

    #[test]
    fn zeta_values() {
        check!(zeta(2.0), PI * PI / 6.0);
        check!(zeta(3.0), 1.202_056_903_159_594_3);
        check!(zeta(0.0), -0.5);
        check!(zeta(-1.0), -1.0 / 12.0);
        check!(zeta(0.5), -1.460_354_508_809_586_8);
        check!(zeta(-2.5), 0.008_516_928_777_850_33, 1e-9);
        assert_eq!(zeta(-4.0), 0.0);
        // The first nontrivial zero.
        let z = zeta_complex(C::new(0.5, 14.134_725_141_734_693));
        assert!(z.abs() < 1e-9, "{z:?}");
        let z = zeta_complex(C::new(2.0, 1.0));
        check!(z.re, 1.150_355_703_254_903, 1e-10);
        check!(z.im, -0.437_530_865_919_607_9, 1e-10);
        let z = zeta_complex(C::new(-1.0, 3.0));
        check!(z.re, 0.274_124_084_675_719_3, 1e-9);
        check!(z.im, 0.058_777_533_260_970_82, 1e-9);
    }

    #[test]
    fn bessel_functions() {
        check!(bessel_j(0.0, 1.0), 0.765_197_686_557_966_6);
        check!(bessel_j(2.0, 10.0), 0.254_630_313_685_120_9);
        check!(bessel_j(0.5, 2.0), (2.0 / (PI * 2.0)).sqrt() * 2f64.sin(), 1e-10);
        check!(bessel_j(1.5, 30.0), -0.027_267_945_711_177_69, 1e-9);
        check!(bessel_j(-0.5, 2.0), (2.0 / (PI * 2.0)).sqrt() * 2f64.cos(), 1e-10);
        check!(bessel_y(0.0, 1.0), 0.088_256_964_215_676_96);
        check!(bessel_y(0.5, 2.0), -(2.0 / (PI * 2.0)).sqrt() * 2f64.cos(), 1e-10);
        check!(bessel_i(0.0, 1.0), 1.266_065_877_752_008_4);
        check!(bessel_i(1.0, 20.0), 4.245_497_338_512_777e7, 1e-10);
        check!(bessel_i(0.5, 2.0), (2.0 / (PI * 2.0)).sqrt() * 2f64.sinh(), 1e-10);
        check!(bessel_k(0.0, 1.0), 0.421_024_438_240_708_3);
        check!(bessel_k(0.5, 2.0), (PI / 4.0).sqrt() * (-2f64).exp(), 1e-10);
        check!(bessel_k(1.0, 0.01), 99.973_894_118_296_25, 1e-10);
    }

    #[test]
    fn airy_functions() {
        check!(airy_ai(0.0), AI0);
        check!(airy_ai(1.0), 0.135_292_416_312_881_4);
        check!(airy_ai(3.0), 0.006_591_139_357_460_72, 1e-9);
        check!(airy_ai(-5.0), 0.350_761_009_024_114, 1e-9);
        check!(airy_bi(1.0), 1.207_423_594_952_871);
        check!(airy_bi(3.0), 14.037_328_963_730_23, 1e-9);
        check!(airy_bi(-5.0), -0.138_369_134_901_600_5, 1e-9);
    }

    #[test]
    fn integrals() {
        check!(si(1.0), 0.946_083_070_367_183);
        check!(si(10.0), 1.658_347_594_218_874);
        check!(si(-3.0), -1.848_652_527_999_468);
        check!(ci(1.0), 0.337_403_922_900_968_1);
        check!(ci(10.0), -0.045_456_433_004_455_37);
        check!(ei(1.0), 1.895_117_816_355_937);
        check!(ei(-1.0), -0.219_383_934_395_520_3);
        check!(ei(50.0), 1.058_563_689_713_169e20, 1e-10);
        check!(li(10.0), 6.165_599_504_787_298);
    }

    #[test]
    fn lambert_w_values() {
        check!(lambert_w(1.0), 0.567_143_290_409_783_8);
        check!(lambert_w(-0.2), -0.259_171_101_819_073_8);
        check!(lambert_w(100.0), 3.385_630_140_290_05);
        check!(lambert_w(-0.36), -0.806_084_315_970_817_6, 1e-9);
        assert!(lambert_w(-1.0).is_nan());
    }

    #[test]
    fn polylog_values() {
        check!(polylog(2.0, 0.5), PI * PI / 12.0 - 2f64.ln().powi(2) / 2.0);
        check!(polylog(2.0, 0.9), 1.299_714_723_004_959, 1e-10);
        check!(polylog(2.0, -1.0), -PI * PI / 12.0, 1e-10);
        check!(polylog(3.0, 0.75), 0.844_425_808_862_204_5, 1e-10);
        check!(polylog(1.0, 0.8), -(0.2f64).ln(), 1e-10);
        check!(polylog(2.0, -3.0), -1.939_375_420_766_708, 1e-10);
        check!(polylog(0.5, 0.7), 1.579_938_318_100_295, 1e-9);
        check!(polylog(2.0, 1.0), PI * PI / 6.0);
    }

    #[test]
    fn elliptic_integrals() {
        check!(elliptic_k(0.0), FRAC_PI_2);
        check!(elliptic_k(0.5), 1.685_750_354_812_596);
        check!(elliptic_e(0.5), 1.467_462_209_339_427);
        check!(elliptic_e(1.0), 1.0);
    }
}
