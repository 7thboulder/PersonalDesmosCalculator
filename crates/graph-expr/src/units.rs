//! Units and physical constants. A unit is written as text in a row, typed
//! with `$` as in `5$m/s$ \cdot 3$h$`, and a row converts its answer when it
//! ends with text such as `in km`. Units become plain SI factors before a
//! row is worked out, so graphs run exactly as before; this module checks
//! that the units agree and picks the unit each answer is shown in.

use std::collections::HashMap;

use crate::ast::{Condition, Expr, Repeat, Statement};
use crate::builtins::Builtin;

/// Powers of the SI base units: metre, kilogram, second, ampere, kelvin,
/// mole and candela.
pub type Dim = [i8; 7];

const NONE: Dim = [0; 7];
const BASE: [&str; 7] = ["m", "kg", "s", "A", "K", "mol", "cd"];

/// A unit as written: `km/h` or `kg m/s^2`.
#[derive(Clone, Debug, PartialEq)]
pub struct Quantity {
    /// How many SI units one of this is.
    pub factor: f64,
    pub dim: Dim,
    /// Each unit named, with its power.
    pub parts: Vec<Part>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub name: String,
    pub power: i8,
    /// One of this unit, to the first power.
    pub factor: f64,
    pub dim: Dim,
    /// A constant such as `c` or `G` rather than a unit to show answers in.
    pub constant: bool,
}

impl Quantity {
    fn has_constant(&self) -> bool {
        self.parts.iter().any(|p| p.constant)
    }

    /// LaTeX for the unit, as the panel shows it after a value.
    pub fn latex(&self) -> String {
        let part = |p: &Part, power: i8| {
            let name = format!("\\text{{{}}}", p.name);
            if power == 1 {
                name
            } else {
                format!("{name}^{{{power}}}")
            }
        };
        let over: Vec<String> = self
            .parts
            .iter()
            .filter(|p| p.power > 0)
            .map(|p| part(p, p.power))
            .collect();
        let under: Vec<String> = self
            .parts
            .iter()
            .filter(|p| p.power < 0)
            .map(|p| part(p, -p.power))
            .collect();
        if over.is_empty() {
            let all: Vec<String> = self.parts.iter().map(|p| part(p, p.power)).collect();
            return all.join("\\cdot ");
        }
        let mut out = over.join("\\cdot ");
        if !under.is_empty() {
            out.push('/');
            if under.len() > 1 {
                out.push_str(&format!("\\left({}\\right)", under.join("\\cdot ")));
            } else {
                out.push_str(&under[0]);
            }
        }
        out
    }

    fn plain(&self) -> String {
        self.parts
            .iter()
            .map(|p| match p.power {
                1 => p.name.clone(),
                n => format!("{}^{n}", p.name),
            })
            .collect::<Vec<_>>()
            .join("·")
    }

    fn from_dim(dim: Dim) -> Quantity {
        let parts = BASE
            .iter()
            .zip(dim)
            .filter(|(_, p)| *p != 0)
            .map(|(name, power)| {
                let mut d = NONE;
                d[BASE.iter().position(|b| b == name).expect("a base unit")] = 1;
                Part {
                    name: (*name).into(),
                    power,
                    factor: 1.0,
                    dim: d,
                    constant: false,
                }
            })
            .collect();
        Quantity {
            factor: 1.0,
            dim,
            parts,
        }
    }
}

/// What a value is shown in: divide by `factor`, then write `latex` after it.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    pub latex: String,
    pub factor: f64,
}

/// A unit or constant: its name, how many SI units it is, its powers of
/// the base units, and whether SI prefixes (k, m, µ…) go in front of it.
struct Entry {
    names: &'static [&'static str],
    factor: f64,
    dim: Dim,
    prefixed: bool,
    constant: bool,
}

const fn d(m: i8, kg: i8, s: i8, a: i8, k: i8, mol: i8, cd: i8) -> Dim {
    [m, kg, s, a, k, mol, cd]
}

const fn unit(names: &'static [&'static str], factor: f64, dim: Dim, prefixed: bool) -> Entry {
    Entry {
        names,
        factor,
        dim,
        prefixed,
        constant: false,
    }
}

const fn constant(names: &'static [&'static str], factor: f64, dim: Dim) -> Entry {
    Entry {
        names,
        factor,
        dim,
        prefixed: false,
        constant: true,
    }
}

const LENGTH: Dim = d(1, 0, 0, 0, 0, 0, 0);
const MASS: Dim = d(0, 1, 0, 0, 0, 0, 0);
const TIME: Dim = d(0, 0, 1, 0, 0, 0, 0);
const AREA: Dim = d(2, 0, 0, 0, 0, 0, 0);
const VOLUME: Dim = d(3, 0, 0, 0, 0, 0, 0);
const SPEED: Dim = d(1, 0, -1, 0, 0, 0, 0);
const ACCEL: Dim = d(1, 0, -2, 0, 0, 0, 0);
const FORCE: Dim = d(1, 1, -2, 0, 0, 0, 0);
const PRESSURE: Dim = d(-1, 1, -2, 0, 0, 0, 0);
const ENERGY: Dim = d(2, 1, -2, 0, 0, 0, 0);
const POWER: Dim = d(2, 1, -3, 0, 0, 0, 0);
const CHARGE: Dim = d(0, 0, 1, 1, 0, 0, 0);
const VOLTAGE: Dim = d(2, 1, -3, -1, 0, 0, 0);
const RESISTANCE: Dim = d(2, 1, -3, -2, 0, 0, 0);
const CAPACITANCE: Dim = d(-2, -1, 4, 2, 0, 0, 0);
const FLUX: Dim = d(2, 1, -2, -1, 0, 0, 0);
const FIELD: Dim = d(0, 1, -2, -1, 0, 0, 0);
const INDUCTANCE: Dim = d(2, 1, -2, -2, 0, 0, 0);
const CONDUCTANCE: Dim = d(-2, -1, 3, 2, 0, 0, 0);
const FREQUENCY: Dim = d(0, 0, -1, 0, 0, 0, 0);

const GALLON: f64 = 3.785411784e-3;
const FLUID_OUNCE: f64 = GALLON / 128.0;

const TABLE: &[Entry] = &[
    // SI base units, and kg's gram.
    unit(&["m"], 1.0, LENGTH, true),
    unit(&["g"], 1e-3, MASS, true),
    unit(&["s", "sec"], 1.0, TIME, true),
    unit(&["A"], 1.0, d(0, 0, 0, 1, 0, 0, 0), true),
    unit(&["K"], 1.0, d(0, 0, 0, 0, 1, 0, 0), true),
    unit(&["mol"], 1.0, d(0, 0, 0, 0, 0, 1, 0), true),
    unit(&["cd"], 1.0, d(0, 0, 0, 0, 0, 0, 1), true),
    // Named SI units.
    unit(&["Hz"], 1.0, FREQUENCY, true),
    unit(&["N"], 1.0, FORCE, true),
    unit(&["Pa"], 1.0, PRESSURE, true),
    unit(&["J"], 1.0, ENERGY, true),
    unit(&["W"], 1.0, POWER, true),
    unit(&["C"], 1.0, CHARGE, true),
    unit(&["V"], 1.0, VOLTAGE, true),
    unit(&["ohm", "Ω"], 1.0, RESISTANCE, true),
    unit(&["F"], 1.0, CAPACITANCE, true),
    unit(&["Wb"], 1.0, FLUX, true),
    unit(&["T"], 1.0, FIELD, true),
    unit(&["H"], 1.0, INDUCTANCE, true),
    unit(&["S"], 1.0, CONDUCTANCE, true),
    // Time.
    unit(&["min"], 60.0, TIME, false),
    unit(&["h", "hr"], 3600.0, TIME, false),
    unit(&["day", "days"], 86400.0, TIME, false),
    unit(&["wk", "week", "weeks"], 604800.0, TIME, false),
    unit(&["yr", "year", "years"], 31_557_600.0, TIME, false),
    // Length.
    unit(&["in", "inch", "inches"], 0.0254, LENGTH, false),
    unit(&["ft", "foot", "feet"], 0.3048, LENGTH, false),
    unit(&["yd"], 0.9144, LENGTH, false),
    unit(&["mi", "mile", "miles"], 1609.344, LENGTH, false),
    unit(&["nmi"], 1852.0, LENGTH, false),
    unit(&["au", "AU"], 149_597_870_700.0, LENGTH, false),
    unit(&["ly"], 9_460_730_472_580_800.0, LENGTH, false),
    unit(&["pc"], 3.085_677_581_491_367e16, LENGTH, true),
    unit(&["angstrom", "Å"], 1e-10, LENGTH, false),
    // Mass.
    unit(&["t", "tonne"], 1000.0, MASS, true),
    unit(&["lb", "lbs"], 0.453_592_37, MASS, false),
    unit(&["oz"], 0.028_349_523_125, MASS, false),
    unit(&["ton"], 907.184_74, MASS, false),
    unit(&["Da", "u"], 1.660_539_066_6e-27, MASS, false),
    // Area and volume.
    unit(&["ha"], 1e4, AREA, false),
    unit(&["acre", "acres"], 4_046.856_422_4, AREA, false),
    unit(&["L", "l"], 1e-3, VOLUME, true),
    unit(&["gal"], GALLON, VOLUME, false),
    unit(&["qt"], GALLON / 4.0, VOLUME, false),
    unit(&["pt"], GALLON / 8.0, VOLUME, false),
    unit(&["cup", "cups"], GALLON / 16.0, VOLUME, false),
    unit(&["floz"], FLUID_OUNCE, VOLUME, false),
    unit(&["tbsp"], FLUID_OUNCE / 2.0, VOLUME, false),
    unit(&["tsp"], FLUID_OUNCE / 6.0, VOLUME, false),
    // Speed.
    unit(&["mph"], 0.447_04, SPEED, false),
    unit(&["kph"], 1.0 / 3.6, SPEED, false),
    unit(&["kn", "knot", "knots"], 1852.0 / 3600.0, SPEED, false),
    // Energy, power and pressure.
    unit(&["eV"], 1.602_176_634e-19, ENERGY, true),
    unit(&["Wh"], 3600.0, ENERGY, true),
    unit(&["cal"], 4.184, ENERGY, true),
    unit(&["Cal", "kcal"], 4184.0, ENERGY, false),
    unit(&["BTU"], 1_055.055_852_62, ENERGY, false),
    unit(&["hp"], 745.699_871_582_270_2, POWER, false),
    unit(&["bar"], 1e5, PRESSURE, true),
    unit(&["atm"], 101_325.0, PRESSURE, false),
    unit(&["psi"], 6_894.757_293_168, PRESSURE, false),
    unit(&["mmHg"], 133.322_387_415, PRESSURE, false),
    unit(&["torr", "Torr"], 101_325.0 / 760.0, PRESSURE, false),
    // Angles have no dimension; degrees are a factor of π/180.
    unit(&["rad"], 1.0, NONE, false),
    unit(&["deg", "°"], std::f64::consts::PI / 180.0, NONE, false),
    unit(&["rev"], std::f64::consts::TAU, NONE, false),
    unit(&["arcmin"], std::f64::consts::PI / 10800.0, NONE, false),
    unit(&["arcsec"], std::f64::consts::PI / 648_000.0, NONE, false),
    // Physical constants (2018 CODATA), and the golden ratio.
    constant(&["c"], 299_792_458.0, SPEED),
    constant(&["G"], 6.674_30e-11, d(3, -1, -2, 0, 0, 0, 0)),
    constant(&["g0", "gn"], 9.806_65, ACCEL),
    constant(
        &["planck", "h_P"],
        6.626_070_15e-34,
        d(2, 1, -1, 0, 0, 0, 0),
    ),
    constant(&["hbar"], 1.054_571_817e-34, d(2, 1, -1, 0, 0, 0, 0)),
    constant(&["e"], 1.602_176_634e-19, CHARGE),
    constant(&["kB", "k_B"], 1.380_649e-23, d(2, 1, -2, 0, -1, 0, 0)),
    constant(&["NA", "N_A"], 6.022_140_76e23, d(0, 0, 0, 0, 0, -1, 0)),
    constant(&["R"], 8.314_462_618, d(2, 1, -2, 0, -1, -1, 0)),
    constant(&["me", "m_e"], 9.109_383_701_5e-31, MASS),
    constant(&["mp", "m_p"], 1.672_621_923_69e-27, MASS),
    constant(&["mn", "m_n"], 1.674_927_498_04e-27, MASS),
    constant(
        &["eps0", "ε0"],
        8.854_187_812_8e-12,
        d(-3, -1, 4, 2, 0, 0, 0),
    ),
    constant(
        &["mu0", "μ0"],
        1.256_637_062_12e-6,
        d(1, 1, -2, -2, 0, 0, 0),
    ),
    constant(&["sigma", "σ"], 5.670_374_419e-8, d(0, 1, -3, 0, -4, 0, 0)),
    constant(&["phi"], 1.618_033_988_749_895, NONE),
];

const PREFIXES: &[(&str, f64)] = &[
    ("Y", 1e24),
    ("Z", 1e21),
    ("E", 1e18),
    ("P", 1e15),
    ("T", 1e12),
    ("G", 1e9),
    ("M", 1e6),
    ("k", 1e3),
    ("h", 1e2),
    ("da", 1e1),
    ("d", 1e-1),
    ("c", 1e-2),
    ("m", 1e-3),
    ("u", 1e-6),
    ("µ", 1e-6),
    ("μ", 1e-6),
    ("n", 1e-9),
    ("p", 1e-12),
    ("f", 1e-15),
    ("a", 1e-18),
];

fn lookup(name: &str) -> Option<Part> {
    let part = |e: &Entry, prefix: f64| Part {
        name: name.to_owned(),
        power: 1,
        factor: e.factor * prefix,
        dim: e.dim,
        constant: e.constant,
    };
    if let Some(e) = TABLE.iter().find(|e| e.names.contains(&name)) {
        return Some(part(e, 1.0));
    }
    PREFIXES.iter().find_map(|(p, scale)| {
        let rest = name.strip_prefix(p)?;
        let e = TABLE
            .iter()
            .find(|e| e.prefixed && e.names.contains(&rest))?;
        Some(part(e, *scale))
    })
}

/// Whether `name` is a unit or constant, alone or with a prefix.
pub fn is_unit(name: &str) -> bool {
    lookup(name).is_some()
}

/// Reads a unit written as text: `km`, `m/s^2`, `kg m^2/s^2`, `J/(mol K)`.
pub fn parse_unit(text: &str) -> Result<Quantity, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut r = UnitReader { chars, pos: 0 };
    let mut parts = Vec::new();
    r.product(&mut parts, 1)?;
    if r.pos < r.chars.len() {
        return Err(format!("'{text}' is not a unit"));
    }
    if parts.is_empty() {
        return Err("Write a unit, like m/s".into());
    }
    // m·m reads as m^2.
    let mut merged: Vec<Part> = Vec::new();
    for p in parts {
        match merged.iter_mut().find(|q| q.name == p.name) {
            Some(q) => q.power += p.power,
            None => merged.push(p),
        }
    }
    merged.retain(|p| p.power != 0);
    let mut q = Quantity {
        factor: 1.0,
        dim: NONE,
        parts: merged,
    };
    for p in &q.parts {
        q.factor *= p.factor.powi(p.power as i32);
        q.dim = add(q.dim, scale(p.dim, p.power));
    }
    Ok(q)
}

struct UnitReader {
    chars: Vec<char>,
    pos: usize,
}

impl UnitReader {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn skip_spaces(&mut self) {
        while self.peek() == Some(' ') {
            self.pos += 1;
        }
    }

    /// Units multiplied and divided, each part's power times `sign`. A `/`
    /// divides by the one unit after it, or by a bracketed group.
    fn product(&mut self, parts: &mut Vec<Part>, sign: i8) -> Result<(), String> {
        let mut divide = false;
        loop {
            self.skip_spaces();
            match self.peek() {
                None | Some(')') => return Ok(()),
                Some('*' | '·' | '.') => {
                    self.pos += 1;
                    continue;
                }
                Some('/') => {
                    self.pos += 1;
                    divide = true;
                    continue;
                }
                _ => {}
            }
            let s = if divide { -sign } else { sign };
            divide = false;
            self.factor(parts, s)?;
        }
    }

    fn factor(&mut self, parts: &mut Vec<Part>, sign: i8) -> Result<(), String> {
        let mut group = Vec::new();
        if self.peek() == Some('(') {
            self.pos += 1;
            self.product(&mut group, 1)?;
            if self.peek() != Some(')') {
                return Err("Unbalanced brackets in a unit".into());
            }
            self.pos += 1;
        } else {
            let start = self.pos;
            while self
                .peek()
                .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '°')
            {
                self.pos += 1;
            }
            let mut name: String = self.chars[start..self.pos].iter().collect();
            if name.is_empty() {
                let c = self.peek().unwrap_or(' ');
                return Err(format!("Unexpected '{c}' in a unit"));
            }
            // Digits belong to the name for g0 and mu0, and are a power
            // otherwise: m2 is m^2.
            let digits_end = (self.pos..self.chars.len())
                .find(|&i| !self.chars[i].is_ascii_digit())
                .unwrap_or(self.chars.len());
            if digits_end > self.pos {
                let with: String = self.chars[start..digits_end].iter().collect();
                if is_unit(&with) {
                    name = with;
                    self.pos = digits_end;
                }
            }
            let part = lookup(&name).ok_or_else(|| format!("'{name}' is not a unit I know"))?;
            group.push(part);
        }
        let power = self.power()?;
        for mut p in group {
            p.power *= power * sign;
            parts.push(p);
        }
        Ok(())
    }

    /// `^2`, `^-1`, `^{-1}` or bare digits after a unit; 1 when there are none.
    fn power(&mut self) -> Result<i8, String> {
        let caret = self.peek() == Some('^');
        if caret {
            self.pos += 1;
        }
        let braced = caret && self.peek() == Some('{');
        if braced {
            self.pos += 1;
        }
        let negative = matches!(self.peek(), Some('-' | '−'));
        if negative {
            self.pos += 1;
        }
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        if start == self.pos {
            if caret || negative {
                return Err("A unit's power must be a whole number, like m^2".into());
            }
            return Ok(1);
        }
        if braced {
            if self.peek() != Some('}') {
                return Err("A unit's power must be a whole number, like m^2".into());
            }
            self.pos += 1;
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        let n: i8 = text
            .parse()
            .map_err(|_| "That power is too large".to_string())?;
        Ok(if negative { -n } else { n })
    }
}

fn add(a: Dim, b: Dim) -> Dim {
    std::array::from_fn(|i| a[i] + b[i])
}

fn sub(a: Dim, b: Dim) -> Dim {
    std::array::from_fn(|i| a[i] - b[i])
}

fn scale(a: Dim, n: i8) -> Dim {
    a.map(|p| p * n)
}

/// A dimension in words for error messages: `m/s`, `kg·m^2/s^2`.
fn describe(dim: Dim) -> String {
    if dim == NONE {
        return "no units".into();
    }
    if let Some(name) = derived_name(dim) {
        return name.into();
    }
    let over: Vec<String> = BASE
        .iter()
        .zip(dim)
        .filter(|(_, p)| *p > 0)
        .map(|(n, p)| {
            if p == 1 {
                n.to_string()
            } else {
                format!("{n}^{p}")
            }
        })
        .collect();
    let under: Vec<String> = BASE
        .iter()
        .zip(dim)
        .filter(|(_, p)| *p < 0)
        .map(|(n, p)| {
            if p == -1 {
                n.to_string()
            } else {
                format!("{n}^{}", -p)
            }
        })
        .collect();
    let top = if over.is_empty() {
        "1".into()
    } else {
        over.join("·")
    };
    if under.is_empty() {
        top
    } else {
        format!("{top}/{}", under.join("·"))
    }
}

fn derived_name(dim: Dim) -> Option<&'static str> {
    const NAMED: &[(&str, Dim)] = &[
        ("N", FORCE),
        ("Pa", PRESSURE),
        ("J", ENERGY),
        ("W", POWER),
        ("C", CHARGE),
        ("V", VOLTAGE),
        ("Ω", RESISTANCE),
        ("F", CAPACITANCE),
        ("Wb", FLUX),
        ("T", FIELD),
        ("H", INDUCTANCE),
        ("S", CONDUCTANCE),
        ("Hz", FREQUENCY),
    ];
    NAMED.iter().find(|(_, d)| *d == dim).map(|(n, _)| *n)
}

/// What the rows define, as written, for following units across rows.
pub struct Defs<'a> {
    vars: HashMap<&'a str, &'a Expr>,
    funcs: HashMap<&'a str, (Vec<&'a str>, &'a Expr)>,
}

impl<'a> Defs<'a> {
    pub fn new(statements: impl Iterator<Item = &'a Statement>) -> Self {
        let mut vars = HashMap::new();
        let mut funcs = HashMap::new();
        for s in statements {
            let Statement::Relation(lhs, crate::ast::Relation::Eq, rhs) = s else {
                continue;
            };
            match lhs {
                Expr::Var(name) => {
                    vars.insert(name.as_str(), rhs);
                }
                Expr::Apply(name, args) => {
                    let params: Option<Vec<&str>> = args
                        .iter()
                        .map(|a| match a {
                            Expr::Var(p) => Some(p.as_str()),
                            _ => None,
                        })
                        .collect();
                    if let Some(params) = params {
                        funcs.insert(name.as_str(), (params, rhs));
                    }
                }
                _ => {}
            }
        }
        Defs { vars, funcs }
    }
}

/// How deep names may refer to names before units stop being followed.
const MAX_DEPTH: usize = 32;

/// The units a row's answer is shown in, or why its units don't agree.
/// None when the row has nothing to do with units.
pub fn row_units(statement: &Statement, defs: &Defs) -> Result<Option<Shown>, String> {
    let (expr, target) = match statement {
        Statement::Expr(e) | Statement::Relation(_, _, e) => split_target(e),
        Statement::Chain(c) => {
            for e in &c.operands {
                if uses_units(e, defs, 0) {
                    check_condition(c, defs)?;
                    break;
                }
            }
            return Ok(None);
        }
        Statement::Regression(..) | Statement::Ode(_) => return Ok(None),
    };
    if let Statement::Relation(lhs, rel, _) = statement {
        let definition = *rel == crate::ast::Relation::Eq;
        match lhs {
            // A function's units depend on what it is given.
            Expr::Apply(..) if definition => return Ok(None),
            Expr::Var(_) if definition => {}
            _ => {
                if uses_units(lhs, defs, 0) || uses_units(expr, defs, 0) {
                    let mut checker = Checker {
                        defs,
                        locals: Vec::new(),
                        depth: 0,
                    };
                    let (a, b) = (checker.dim(lhs)?, checker.dim(expr)?);
                    checker.merge(a, b, "compare")?;
                }
                return Ok(None);
            }
        }
    }
    if target.is_none() && !uses_units(expr, defs, 0) {
        return Ok(None);
    }
    let mut checker = Checker {
        defs,
        locals: Vec::new(),
        depth: 0,
    };
    let dim = checker.dim(expr)?.unwrap_or(NONE);
    if let Some(target) = target {
        if target.dim != dim {
            return Err(format!(
                "Can't convert {} to {}",
                describe(dim),
                target.plain()
            ));
        }
        return Ok(Some(Shown {
            latex: target.latex(),
            factor: target.factor,
        }));
    }
    if dim == NONE {
        return Ok(None);
    }
    let mut written = Vec::new();
    collect(expr, defs, &mut written, 0);
    let chosen = written
        .into_iter()
        .find(|q| q.dim == dim)
        .unwrap_or_else(|| match derived_name(dim) {
            Some(name) => parse_unit(if name == "Ω" { "ohm" } else { name })
                .map(|mut q| {
                    q.parts[0].name = name.into();
                    q
                })
                .expect("a named SI unit"),
            None => Quantity::from_dim(dim),
        });
    Ok(Some(Shown {
        latex: chosen.latex(),
        factor: chosen.factor,
    }))
}

fn check_condition(c: &Condition, defs: &Defs) -> Result<(), String> {
    let mut checker = Checker {
        defs,
        locals: Vec::new(),
        depth: 0,
    };
    let mut dims = Vec::new();
    for e in &c.operands {
        dims.push(checker.dim(e)?);
    }
    dims.into_iter()
        .try_fold(None, |acc, d| checker.merge(acc, d, "compare"))?;
    Ok(())
}

fn split_target(e: &Expr) -> (&Expr, Option<&Quantity>) {
    match e {
        Expr::Convert(inner, q) => (inner, Some(q)),
        e => (e, None),
    }
}

/// Whether an expression has a unit in it, directly or through the names
/// it reads.
fn uses_units(e: &Expr, defs: &Defs, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    let mut found = false;
    e.walk(&mut |x| match x {
        Expr::Unit(_) | Expr::Convert(..) => found = true,
        Expr::Var(n) => {
            if let Some(v) = defs.vars.get(n.as_str()) {
                found |= uses_units(v, defs, depth + 1);
            }
        }
        Expr::Apply(n, _) => {
            if let Some((_, body)) = defs.funcs.get(n.as_str()) {
                found |= uses_units(body, defs, depth + 1);
            } else if let Some(v) = defs.vars.get(n.as_str()) {
                found |= uses_units(v, defs, depth + 1);
            }
        }
        _ => {}
    });
    found
}

/// Units written in an expression and the rows it reads, in order: each
/// whole unit, then the units it is made of. Constants are left out, since
/// `3c` is better shown in m/s.
fn collect(e: &Expr, defs: &Defs, out: &mut Vec<Quantity>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    e.walk(&mut |x| match x {
        Expr::Unit(q) if !q.has_constant() => {
            out.push((**q).clone());
            for p in &q.parts {
                out.push(Quantity {
                    factor: p.factor,
                    dim: p.dim,
                    parts: vec![Part {
                        power: 1,
                        ..p.clone()
                    }],
                });
            }
        }
        Expr::Var(n) => {
            if let Some(v) = defs.vars.get(n.as_str()) {
                collect(v, defs, out, depth + 1);
            }
        }
        Expr::Apply(n, _) => {
            if let Some((_, body)) = defs.funcs.get(n.as_str()) {
                collect(body, defs, out, depth + 1);
            }
        }
        _ => {}
    });
}

/// Works out the dimension of an expression. None means unknown, which
/// agrees with anything; plain numbers have no units.
struct Checker<'a, 'b> {
    defs: &'b Defs<'a>,
    /// Names bound inside the expression, such as a sum's index or a
    /// function's parameters.
    locals: Vec<(String, Option<Dim>)>,
    depth: usize,
}

type D = Option<Dim>;

impl Checker<'_, '_> {
    fn merge(&self, a: D, b: D, verb: &str) -> Result<D, String> {
        match (a, b) {
            (Some(a), Some(b)) if a != b => {
                Err(format!("Can't {verb} {} and {}", describe(a), describe(b)))
            }
            (Some(a), _) => Ok(Some(a)),
            (None, b) => Ok(b),
        }
    }

    fn plain(&self, d: D, what: &str) -> Result<D, String> {
        match d {
            Some(d) if d != NONE => Err(format!(
                "{what} needs a number without units, not {}",
                describe(d)
            )),
            _ => Ok(Some(NONE)),
        }
    }

    fn with_locals<T>(&mut self, bound: Vec<(String, D)>, f: impl FnOnce(&mut Self) -> T) -> T {
        let n = self.locals.len();
        self.locals.extend(bound);
        let out = f(self);
        self.locals.truncate(n);
        out
    }

    fn name(&mut self, name: &str) -> Result<D, String> {
        if let Some((_, d)) = self.locals.iter().rev().find(|(n, _)| n == name) {
            return Ok(*d);
        }
        match self.defs.vars.get(name) {
            Some(e) if self.depth < MAX_DEPTH => {
                // A definition is read in its own scope.
                let saved = std::mem::take(&mut self.locals);
                self.depth += 1;
                let out = self.dim(e);
                self.depth -= 1;
                self.locals = saved;
                out
            }
            Some(_) => Ok(None),
            None => Ok(Some(NONE)),
        }
    }

    fn dim(&mut self, e: &Expr) -> Result<D, String> {
        Ok(match e {
            Expr::Num(_) => Some(NONE),
            Expr::Unit(q) => Some(q.dim),
            Expr::Convert(inner, _) => self.dim(inner)?,
            Expr::Var(name) => self.name(name)?,
            Expr::Neg(a) => self.dim(a)?,
            Expr::Add(a, b) | Expr::Sub(a, b) => {
                let (a, b) = (self.dim(a)?, self.dim(b)?);
                self.merge(a, b, "add")?
            }
            Expr::Mul(a, b) => match (self.dim(a)?, self.dim(b)?) {
                (Some(a), Some(b)) => Some(add(a, b)),
                _ => None,
            },
            Expr::Div(a, b) => match (self.dim(a)?, self.dim(b)?) {
                (Some(a), Some(b)) => Some(sub(a, b)),
                _ => None,
            },
            Expr::Pow(base, exp) => {
                let exp_dim = self.dim(exp)?;
                self.plain(exp_dim, "A power")?;
                match self.dim(base)? {
                    Some(b) if b != NONE => {
                        let n = constant_value(exp)
                            .ok_or("A unit can only be raised to a fixed power, like m^2")?;
                        Some(power(b, n)?)
                    }
                    other => other,
                }
            }
            Expr::Factorial(a) => {
                let d = self.dim(a)?;
                self.plain(d, "A factorial")?
            }
            Expr::Call(f, args) => self.call(*f, args)?,
            Expr::Apply(name, args) => {
                if let Some((params, body)) = self.defs.funcs.get(name.as_str()) {
                    let (params, body) = (params.clone(), *body);
                    if self.depth >= MAX_DEPTH || params.len() != args.len() {
                        return Ok(None);
                    }
                    let mut bound = Vec::new();
                    for (p, a) in params.iter().zip(args) {
                        bound.push((p.to_string(), self.dim(a)?));
                    }
                    let saved = std::mem::replace(&mut self.locals, bound);
                    self.depth += 1;
                    let out = self.dim(body);
                    self.depth -= 1;
                    self.locals = saved;
                    out?
                } else {
                    // `a(x+1)` multiplies.
                    let mut d = self.name(name)?;
                    for a in args {
                        d = match (d, self.dim(a)?) {
                            (Some(a), Some(b)) => Some(add(a, b)),
                            _ => None,
                        };
                    }
                    d
                }
            }
            Expr::List(items) => {
                let mut d = None;
                for item in items {
                    let item = self.dim(item)?;
                    d = self.merge(d, item, "list")?;
                }
                d
            }
            Expr::Range(a, b, c) => {
                let mut d = self.dim(a)?;
                if let Some(b) = b {
                    let b = self.dim(b)?;
                    d = self.merge(d, b, "list")?;
                }
                let c = self.dim(c)?;
                self.merge(d, c, "list")?
            }
            Expr::Index(list, _) | Expr::Filter(list, _) => self.dim(list)?,
            Expr::Piecewise(branches, otherwise) => {
                let mut d = None;
                for (condition, value) in branches {
                    let mut c = None;
                    for operand in &condition.operands {
                        let o = self.dim(operand)?;
                        c = self.merge(c, o, "compare")?;
                    }
                    let v = self.dim(value)?;
                    d = self.merge(d, v, "choose between")?;
                }
                if let Some(o) = otherwise {
                    let o = self.dim(o)?;
                    d = self.merge(d, o, "choose between")?;
                }
                d
            }
            Expr::Repeat(kind, var, lo, hi, body) => {
                let (lo, hi) = (self.dim(lo)?, self.dim(hi)?);
                let index = self.merge(lo, hi, "count from")?;
                let body = self.with_locals(vec![(var.clone(), index)], |c| c.dim(body))?;
                match kind {
                    Repeat::Sum => body,
                    Repeat::Product => self.plain(body, "A product")?,
                }
            }
            Expr::Integral(var, lo, hi, body) => {
                let (lo, hi) = (self.dim(lo)?, self.dim(hi)?);
                let v = self.merge(lo, hi, "integrate from")?;
                let body = self.with_locals(vec![(var.clone(), v)], |c| c.dim(body))?;
                match (body, v) {
                    (Some(b), Some(v)) => Some(add(b, v)),
                    _ => None,
                }
            }
            Expr::Derivative(var, body) => {
                let v = self.name(var)?;
                match (self.dim(body)?, v) {
                    (Some(b), Some(v)) => Some(sub(b, v)),
                    _ => None,
                }
            }
            Expr::Comprehension(body, generators) => {
                let mut bound = Vec::new();
                for (name, list) in generators {
                    bound.push((name.clone(), self.dim(list)?));
                }
                self.with_locals(bound, |c| c.dim(body))?
            }
            // Points, shapes, primes and actions aren't followed.
            Expr::Point(..)
            | Expr::Point3(..)
            | Expr::Coord(..)
            | Expr::Prime(..)
            | Expr::Shape(..)
            | Expr::Update(..)
            | Expr::Actions(_) => None,
        })
    }

    fn call(&mut self, f: Builtin, args: &[Expr]) -> Result<D, String> {
        use Builtin::*;
        let mut dims = Vec::new();
        for a in args {
            dims.push(self.dim(a)?);
        }
        let first = dims.first().copied().flatten();
        Ok(match f {
            Abs | Floor | Ceil | Round | Min | Max | Mean | Median | Total => {
                let mut d = None;
                for x in dims {
                    d = self.merge(d, x, "compare")?;
                }
                d
            }
            Mod => {
                let d = dims.get(1).copied().flatten();
                self.merge(first, d, "take the remainder of")?
            }
            Sqrt => match first {
                Some(d) if d != NONE => Some(power(d, 0.5)?),
                other => other,
            },
            Length | Sign => Some(NONE),
            _ => {
                for x in dims {
                    self.plain(x, f.name())?;
                }
                Some(NONE)
            }
        })
    }
}

fn power(d: Dim, n: f64) -> Result<Dim, String> {
    let mut out = NONE;
    for (o, p) in out.iter_mut().zip(d) {
        let v = p as f64 * n;
        if v.fract() != 0.0 || v.abs() > 100.0 {
            return Err(format!("Can't raise {} to the power {n}", describe(d)));
        }
        *o = v as i8;
    }
    Ok(out)
}

/// The value of a power written with numbers only: `2`, `-1`, `\frac{1}{2}`.
fn constant_value(e: &Expr) -> Option<f64> {
    Some(match e {
        Expr::Num(v) => *v,
        Expr::Neg(a) => -constant_value(a)?,
        Expr::Add(a, b) => constant_value(a)? + constant_value(b)?,
        Expr::Sub(a, b) => constant_value(a)? - constant_value(b)?,
        Expr::Mul(a, b) => constant_value(a)? * constant_value(b)?,
        Expr::Div(a, b) => constant_value(a)? / constant_value(b)?,
        _ => return None,
    })
}

/// Replaces every unit by its SI factor and drops conversions, which only
/// change how the answer is shown.
pub fn lower(e: Expr) -> Expr {
    let b = |e: Box<Expr>| Box::new(lower(*e));
    let all = |v: Vec<Expr>| v.into_iter().map(lower).collect::<Vec<_>>();
    let cond = |c: Condition| Condition {
        operands: c.operands.into_iter().map(lower).collect(),
        relations: c.relations,
    };
    match e {
        Expr::Unit(q) => Expr::Num(q.factor),
        Expr::Convert(inner, _) => lower(*inner),
        Expr::Num(_) | Expr::Var(_) => e,
        Expr::Neg(a) => Expr::Neg(b(a)),
        Expr::Factorial(a) => Expr::Factorial(b(a)),
        Expr::Coord(a, c) => Expr::Coord(b(a), c),
        Expr::Add(x, y) => Expr::Add(b(x), b(y)),
        Expr::Sub(x, y) => Expr::Sub(b(x), b(y)),
        Expr::Mul(x, y) => Expr::Mul(b(x), b(y)),
        Expr::Div(x, y) => Expr::Div(b(x), b(y)),
        Expr::Pow(x, y) => Expr::Pow(b(x), b(y)),
        Expr::Point(x, y) => Expr::Point(b(x), b(y)),
        Expr::Index(x, y) => Expr::Index(b(x), b(y)),
        Expr::Filter(x, y) => Expr::Filter(b(x), b(y)),
        Expr::Point3(x, y, z) => Expr::Point3(b(x), b(y), b(z)),
        Expr::Call(f, args) => Expr::Call(f, all(args)),
        Expr::Apply(n, args) => Expr::Apply(n, all(args)),
        Expr::Shape(f, args) => Expr::Shape(f, all(args)),
        Expr::Actions(args) => Expr::Actions(all(args)),
        Expr::Prime(n, k, args) => Expr::Prime(n, k, all(args)),
        Expr::List(items) => Expr::List(all(items)),
        Expr::Range(x, y, z) => Expr::Range(b(x), y.map(b), b(z)),
        Expr::Comprehension(body, generators) => Expr::Comprehension(
            b(body),
            generators.into_iter().map(|(n, l)| (n, lower(l))).collect(),
        ),
        Expr::Piecewise(branches, otherwise) => Expr::Piecewise(
            branches
                .into_iter()
                .map(|(c, v)| (cond(c), lower(v)))
                .collect(),
            otherwise.map(b),
        ),
        Expr::Derivative(v, body) => Expr::Derivative(v, b(body)),
        Expr::Repeat(k, v, lo, hi, body) => Expr::Repeat(k, v, b(lo), b(hi), b(body)),
        Expr::Integral(v, lo, hi, body) => Expr::Integral(v, b(lo), b(hi), b(body)),
        Expr::Update(n, value) => Expr::Update(n, b(value)),
    }
}

pub fn lower_statement(s: Statement) -> Statement {
    match s {
        Statement::Expr(e) => Statement::Expr(lower(e)),
        Statement::Relation(a, r, b) => Statement::Relation(lower(a), r, lower(b)),
        Statement::Chain(c) => Statement::Chain(Condition {
            operands: c.operands.into_iter().map(lower).collect(),
            relations: c.relations,
        }),
        Statement::Regression(a, b) => Statement::Regression(lower(a), lower(b)),
        Statement::Ode(e) => Statement::Ode(lower(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_units() {
        let q = parse_unit("km/h").unwrap();
        assert!((q.factor - 1000.0 / 3600.0).abs() < 1e-12);
        assert_eq!(q.dim, SPEED);
        assert_eq!(parse_unit("kg m/s^2").unwrap().dim, FORCE);
        assert_eq!(parse_unit("kg·m^2·s^-2").unwrap().dim, ENERGY);
        assert_eq!(
            parse_unit("J/(mol K)").unwrap().dim,
            d(2, 1, -2, 0, -1, -1, 0)
        );
        assert_eq!(parse_unit("m2").unwrap().dim, AREA);
        assert_eq!(parse_unit("g0").unwrap().dim, ACCEL);
        assert!((parse_unit("mm").unwrap().factor - 1e-3).abs() < 1e-18);
        assert!((parse_unit("min").unwrap().factor - 60.0).abs() < 1e-12);
        assert!((parse_unit("ms").unwrap().factor - 1e-3).abs() < 1e-18);
        assert!((parse_unit("kWh").unwrap().factor - 3.6e6).abs() < 1e-6);
        assert!(parse_unit("furlongs").is_err());
        assert!(parse_unit("m^").is_err());
    }

    #[test]
    fn latex_of_units() {
        assert_eq!(parse_unit("km/h").unwrap().latex(), "\\text{km}/\\text{h}");
        assert_eq!(
            parse_unit("m/s^2").unwrap().latex(),
            "\\text{m}/\\text{s}^{2}"
        );
        assert_eq!(parse_unit("s^-1").unwrap().latex(), "\\text{s}^{-1}");
    }
}
