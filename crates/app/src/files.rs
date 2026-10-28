//! Saving and opening graphs. A graph file is JSON holding the panel's rows
//! (as the panel sends them, minus their ids) and the view. The session is
//! also autosaved, so closing the window never loses work and the next
//! launch picks up where the last one left off.

// Only the Windows window saves and opens outside tests.
#![cfg_attr(not(windows), allow(dead_code))]

use graph_core::{View3, Viewport};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const EXTENSION: &str = "gcalc";
const FORMAT: &str = "graphcalc";
const VERSION: u64 = 1;

/// The open graph: where it lives on disk and whether it has changed since.
#[derive(Default)]
pub struct Document {
    pub path: Option<PathBuf>,
    /// The rows as last saved or opened, in comparable form.
    saved: String,
    /// The rows as the panel last sent them, and the mode.
    current: String,
    rows: String,
    three_d: bool,
    /// The next rows to arrive are the freshly loaded graph, not an edit.
    loading: bool,
}

/// Rows in a form that compares equal across loads: ids are dropped, since
/// the panel numbers rows afresh each time.
fn strip_ids(rows: &Value) -> Value {
    Value::Array(
        rows.as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(|r| {
                let mut r = r.clone();
                if let Some(o) = r.as_object_mut() {
                    o.remove("id");
                }
                r
            })
            .collect(),
    )
}

impl Document {
    /// The panel sent its rows.
    pub fn rows_changed(&mut self, rows: &Value) {
        self.rows = strip_ids(rows).to_string();
        self.refresh();
        if self.loading {
            self.loading = false;
            self.saved = self.current.clone();
        }
    }

    /// The graph was switched between 2D and 3D, which is a change too.
    pub fn mode_changed(&mut self, three_d: bool) {
        self.three_d = three_d;
        self.refresh();
    }

    fn refresh(&mut self) {
        self.current = format!("{}{}", self.rows, if self.three_d { " 3d" } else { "" });
    }

    pub fn dirty(&self) -> bool {
        self.current != self.saved
    }

    /// The rows now in the panel were just written to `path`.
    pub fn saved_to(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.saved = self.current.clone();
    }

    /// A graph is being loaded into the panel from `path` (None for a new
    /// graph); its first rows count as unchanged.
    pub fn loading(&mut self, path: Option<PathBuf>) {
        self.path = path;
        self.loading = true;
    }

    /// The window title: the file's name, starred when changed, as Windows
    /// editors do.
    pub fn title(&self) -> String {
        let name = self
            .path
            .as_deref()
            .and_then(Path::file_stem)
            .map_or("Untitled graph".into(), |s| {
                s.to_string_lossy().into_owned()
            });
        let star = if self.dirty() { "*" } else { "" };
        format!("{star}{name} - Graphing Calculator")
    }
}

/// A graph file's contents.
pub fn to_file(rows: &Value, vp: &Viewport, space: &Space) -> String {
    let mut file = json!({
        "format": FORMAT,
        "version": VERSION,
        "viewport": {
            "center": vp.center,
            "width": vp.units_per_px * vp.width_px,
        },
        "rows": strip_ids(rows),
    });
    if space.three_d {
        file["mode"] = json!("3d");
    }
    if let Some(b) = &space.view {
        file["box"] = json!({
            "center": b.center,
            "half": b.half,
            "yaw": b.yaw,
            "pitch": b.pitch,
        });
    }
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// Whether a graph is 3D, and how its 3D box was last shown.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Space {
    pub three_d: bool,
    pub view: Option<BoxView>,
}

/// A saved 3D view: the box's range and the camera's angles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxView {
    pub center: [f64; 3],
    pub half: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
}

impl BoxView {
    pub fn of(v: &View3) -> Self {
        Self {
            center: v.center,
            half: v.half,
            yaw: v.yaw,
            pitch: v.pitch,
        }
    }

    pub fn apply(&self, v: &mut View3) {
        v.center = self.center;
        v.half = self.half;
        v.yaw = self.yaw;
        v.pitch = self.pitch;
    }
}

/// The mode and 3D view of a graph file; a Desmos graph is 2D.
pub fn space_of(text: &str) -> Space {
    if let Some(state) = desmos_state(text) {
        return desmos_space(&state);
    }
    let Ok(file) = serde_json::from_str::<Value>(text) else {
        return Space::default();
    };
    if file["format"] != FORMAT {
        return Space::default();
    }
    let b = &file["box"];
    let three = |v: &Value| -> Option<[f64; 3]> {
        let a = [v[0].as_f64()?, v[1].as_f64()?, v[2].as_f64()?];
        a.iter().all(|x| x.is_finite()).then_some(a)
    };
    let view = match (
        three(&b["center"]),
        three(&b["half"]),
        b["yaw"].as_f64(),
        b["pitch"].as_f64(),
    ) {
        (Some(center), Some(half), Some(yaw), Some(pitch)) if half.iter().all(|h| *h > 0.0) => {
            Some(BoxView {
                center,
                half,
                yaw,
                pitch,
            })
        }
        _ => None,
    };
    Space {
        three_d: file["mode"] == "3d",
        view,
    }
}

/// Whether a Desmos graph is from Desmos 3D, and its box. Desmos 3D marks
/// its graphs with `threeDMode` and gives the viewport a z range.
fn desmos_space(state: &Value) -> Space {
    let g = &state["graph"];
    let v = &g["viewport"];
    let three_d = g["threeDMode"].as_bool().unwrap_or(false)
        || (v["zmin"].is_number() && v["zmax"].is_number())
        || state["product"].as_str().is_some_and(|p| p.contains("3d"));
    let range = |lo: &str, hi: &str| -> Option<(f64, f64)> {
        let (a, b) = (v[lo].as_f64()?, v[hi].as_f64()?);
        (b > a && (b - a).is_finite()).then_some((a, b))
    };
    let view = match (
        range("xmin", "xmax"),
        range("ymin", "ymax"),
        range("zmin", "zmax"),
    ) {
        (Some(x), Some(y), Some(z)) if three_d => {
            let mut box3 = View3::new(1.0, 1.0);
            box3.center = [(x.0 + x.1) / 2.0, (y.0 + y.1) / 2.0, (z.0 + z.1) / 2.0];
            box3.half = [(x.1 - x.0) / 2.0, (y.1 - y.0) / 2.0, (z.1 - z.0) / 2.0];
            // worldRotation3D is a 3×3 matrix by rows; minus its first
            // column points toward the camera.
            let m: Vec<f64> = g["worldRotation3D"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_f64)
                .collect();
            if let [m0, _, _, m3, _, _, m6, _, _] = m[..] {
                let toward = [-m0, -m3, -m6];
                let len = toward.iter().map(|c| c * c).sum::<f64>().sqrt();
                if len > 1e-9 && len.is_finite() {
                    box3.yaw = toward[1].atan2(toward[0]);
                    box3.pitch = (toward[2] / len).clamp(-1.0, 1.0).asin();
                }
            }
            Some(BoxView::of(&box3))
        }
        _ => None,
    };
    Space { three_d, view }
}

/// A saved view: the world point at the centre and the width shown.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub center: [f64; 2],
    pub width: f64,
}

impl View {
    /// Shows this view in `vp`, keeping its pixel size.
    pub fn apply(&self, vp: &mut Viewport) {
        vp.center = self.center;
        vp.units_per_px = self.width / vp.width_px;
    }
}

/// Reads a graph file: its rows for the panel, and its view.
pub fn from_file(text: &str) -> Result<(Value, Option<View>), String> {
    let file: Value =
        serde_json::from_str(text).map_err(|e| format!("This isn't a graph file ({e})."))?;
    if file["format"] != FORMAT {
        return Err("This isn't a graph file.".into());
    }
    if file["version"].as_u64().is_none_or(|v| v > VERSION) {
        return Err("This graph was saved by a newer version of the calculator.".into());
    }
    let rows = file["rows"].clone();
    if !rows.is_array() {
        return Err("This graph file is damaged: it has no rows.".into());
    }
    let v = &file["viewport"];
    let view = match (
        v["center"][0].as_f64(),
        v["center"][1].as_f64(),
        v["width"].as_f64(),
    ) {
        (Some(x), Some(y), Some(w)) if w.is_finite() && w > 0.0 => Some(View {
            center: [x, y],
            width: w,
        }),
        _ => None,
    };
    Ok((rows, view))
}

/// Reads any graph the app can open: its own files, or a Desmos graph state
/// (what `Calc.getState()` gives, saved as JSON). The flag says the graph
/// was imported, so saving it should ask for a new file rather than
/// overwrite the Desmos one.
pub fn open_any(text: &str) -> Result<(Value, Option<View>, bool), String> {
    if let Some(state) = desmos_state(text) {
        let (rows, view) = from_desmos(&state);
        return Ok((rows, view, true));
    }
    from_file(text).map(|(rows, view)| (rows, view, false))
}

/// The graph state in text from Desmos: what `Calc.getState()` gives, or
/// the `{"state": ...}` reply desmos.com sends for a saved graph.
pub fn desmos_state(text: &str) -> Option<Value> {
    let mut file = serde_json::from_str::<Value>(text.trim()).ok()?;
    if file["expressions"]["list"].is_array() {
        return Some(file);
    }
    let state = file.get_mut("state")?.take();
    state["expressions"]["list"].is_array().then_some(state)
}

/// The URL to ask desmos.com for, when the text is a link to a saved graph
/// such as `https://www.desmos.com/calculator/iotrkvyz1u` or
/// `https://www.desmos.com/3d/abc123`.
pub fn desmos_link(text: &str) -> Option<String> {
    let text = text.trim();
    let (kind, at) = ["calculator", "3d"]
        .into_iter()
        .find_map(|kind| Some((kind, text.find(&format!("desmos.com/{kind}/"))?)))?;
    let rest = &text[at + "desmos.com/".len() + kind.len() + 1..];
    let hash: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    let prefix = &text[..text.find("desmos.com").unwrap_or(0)];
    let host = prefix
        .strip_prefix("https://")
        .or_else(|| prefix.strip_prefix("http://"))
        .unwrap_or(prefix);
    let plain = host.is_empty() || host == "www.";
    (!hash.is_empty() && plain && !text.contains(char::is_whitespace))
        .then(|| format!("https://www.desmos.com/{kind}/{hash}"))
}

/// A number written as plain latex, such as `-2.5` (what Desmos stores for
/// most settings).
fn latex_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|x| x.is_finite()),
        _ => None,
    }
}

/// The style fields the panel keeps, from a Desmos expression or table
/// column. Settings Desmos left out stay at the panel's defaults.
fn desmos_style(e: &Value) -> serde_json::Map<String, Value> {
    let mut out = serde_json::Map::new();
    if let Some(c) = e["color"].as_str() {
        out.insert("color".into(), json!(c));
    }
    if let Some(h) = e["hidden"].as_bool() {
        out.insert("hidden".into(), json!(h));
    }
    let style = match e["lineStyle"].as_str() {
        Some("DASHED") => Some("dashed"),
        Some("DOTTED") => Some("dotted"),
        Some("SOLID") => Some("solid"),
        _ => None,
    };
    if let Some(s) = style {
        out.insert("style".into(), json!(s));
    }
    let point = match e["pointStyle"].as_str() {
        Some("OPEN") => Some("open"),
        Some("CROSS") => Some("cross"),
        Some("POINT") => Some("filled"),
        _ => None,
    };
    if let Some(p) = point {
        out.insert("pointStyle".into(), json!(p));
    }
    for (from, to) in [
        ("lineWidth", "width"),
        ("pointSize", "pointSize"),
        ("lineOpacity", "opacity"),
        ("fillOpacity", "fillOpacity"),
    ] {
        if let Some(x) = latex_number(&e[from]) {
            out.insert(to.into(), json!(x));
        }
    }
    if let Some(b) = e["points"].as_bool() {
        out.insert("dots".into(), json!(b));
    }
    if let Some(b) = e["lines"].as_bool() {
        out.insert("connect".into(), json!(b));
    }
    out
}

/// Converts a Desmos graph state to the panel's rows and view. Expressions,
/// tables and the ticker come across; folders, notes and images, which the
/// calculator has no use for, are left out.
fn from_desmos(state: &Value) -> (Value, Option<View>) {
    let mut rows = Vec::new();
    let ticker = &state["expressions"]["ticker"];
    if let Some(handler) = ticker["handlerLatex"].as_str() {
        rows.push(json!({
            "latex": handler,
            "hidden": true,
            "ticker": {
                "step": ticker["minStepLatex"].as_str().unwrap_or("0"),
                "playing": false,
            },
        }));
    }
    for e in state["expressions"]["list"]
        .as_array()
        .into_iter()
        .flatten()
    {
        match e["type"].as_str() {
            Some("expression") => {
                let mut row = desmos_style(e);
                row.insert("latex".into(), json!(e["latex"].as_str().unwrap_or("")));
                let s = &e["slider"];
                if s.is_object() {
                    let min = latex_number(&s["min"]).unwrap_or(-10.0);
                    let max = latex_number(&s["max"]).unwrap_or(10.0);
                    let step = latex_number(&s["step"]).filter(|x| *x > 0.0);
                    let mut slider = json!({"min": min, "max": max.max(min), "step": step});
                    let looping = match s["loopMode"].as_str() {
                        Some("LOOP_FORWARD") => Some("forward"),
                        Some("PLAY_ONCE") => Some("once"),
                        Some("PLAY_INDEFINITELY") => Some("forever"),
                        _ => None,
                    };
                    if let Some(l) = looping {
                        slider["loop"] = json!(l);
                    }
                    // Desmos plays at 1x over 4 seconds; its period sets the speed.
                    if let Some(period) = s["animationPeriod"].as_f64().filter(|p| *p > 0.0) {
                        slider["speed"] = json!(4000.0 / period);
                    }
                    row.insert("slider".into(), slider);
                }
                let d = &e["parametricDomain"];
                if let (Some(a), Some(b)) = (d["min"].as_str(), d["max"].as_str()) {
                    row.insert("domain".into(), json!([a, b]));
                }
                rows.push(Value::Object(row));
            }
            Some("table") => {
                let columns: Vec<Value> = e["columns"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|c| {
                        let mut col = desmos_style(c);
                        col.insert("header".into(), json!(c["latex"].as_str().unwrap_or("")));
                        let cells: Vec<Value> = c["values"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .map(|v| json!(v.as_str().unwrap_or("")))
                            .collect();
                        col.insert("cells".into(), Value::Array(cells));
                        Value::Object(col)
                    })
                    .collect();
                rows.push(json!({"latex": "", "table": columns}));
            }
            Some("text") => {
                rows.push(json!({"latex": "", "note": e["text"].as_str().unwrap_or("")}));
            }
            _ => {}
        }
    }
    let v = &state["graph"]["viewport"];
    let view = match (
        v["xmin"].as_f64(),
        v["xmax"].as_f64(),
        v["ymin"].as_f64(),
        v["ymax"].as_f64(),
    ) {
        (Some(x0), Some(x1), Some(y0), Some(y1)) if x1 > x0 && (x1 - x0).is_finite() => {
            Some(View {
                center: [(x0 + x1) / 2.0, (y0 + y1) / 2.0],
                width: x1 - x0,
            })
        }
        _ => None,
    };
    (Value::Array(rows), view)
}

/// The row settings `to_desmos` knows how to carry over. A row setting
/// outside this list is an app-only feature: it is left out of the export
/// with a warning, so a new kind of row should be added here only once it
/// has a Desmos form.
const DESMOS_ROW_KEYS: &[&str] = &[
    "id",
    "latex",
    "color",
    "hidden",
    "style",
    "width",
    "opacity",
    "fillOpacity",
    "pointStyle",
    "pointSize",
    "dots",
    "connect",
    "domain",
    "slider",
    "ticker",
    "note",
    "table",
];

/// Function names Desmos (2D or 3D) knows, as `\operatorname{...}` writes
/// them. A name outside this list still goes across, with a warning that
/// Desmos may not know it.
#[rustfmt::skip]
const DESMOS_FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "csc", "sec", "cot", "arcsin", "arccos", "arctan", "arccsc",
    "arcsec", "arccot", "sinh", "cosh", "tanh", "csch", "sech", "coth", "arcsinh", "arccosh",
    "arctanh", "arccsch", "arcsech", "arccoth", "arsinh", "arcosh", "artanh", "arcsch",
    "arsech", "arcoth", "ln", "log", "exp", "abs", "floor", "ceil", "round", "sign", "sgn",
    "signum", "mod", "min", "max", "gcd", "gcf", "lcm", "nCr", "nPr", "mean", "median",
    "total", "length", "count", "sort", "shuffle", "join", "unique", "stdev", "stddev",
    "stdevp", "stddevp", "var", "variance", "varp", "cov", "covp", "corr", "spearman",
    "quantile", "quartile", "mad", "random", "histogram", "dotplot", "boxplot", "ttest",
    "tscore", "ittest", "normaldist", "tdist", "poissondist", "binomialdist",
    "uniformdist", "pdf", "cdf", "erf", "real", "imag", "conj", "arg", "polygon",
    "distance", "midpoint", "rgb", "hsv", "okhsv", "oklab", "oklch", "tone", "segment",
    "circle", "arc", "line", "ray", "vector", "point", "angle", "directedangle",
    "perpendicular", "parallel", "intersection", "translate", "rotate", "dilate",
    "reflect", "glider", "start", "end", "center", "radius", "vertices", "width", "height",
    "sphere", "cone", "cylinder", "triangle", "cross", "dot", "nthroot", "factorial",
    "for", "with", "sum", "prod", "repeat",
];

/// Desmos's settings as latex: plain numbers, with no trailing `.0`.
fn desmos_number(x: f64) -> String {
    format!("{x}")
}

/// Desmos's style fields for a row or table column, leaving out what is
/// already the default so the export reads like Desmos's own.
fn desmos_style_of(r: &Value, out: &mut serde_json::Map<String, Value>) {
    if let Some(c) = r["color"].as_str() {
        out.insert("color".into(), json!(c));
    }
    if r["hidden"].as_bool() == Some(true) {
        out.insert("hidden".into(), json!(true));
    }
    let line = match r["style"].as_str() {
        Some("dashed") => Some("DASHED"),
        Some("dotted") => Some("DOTTED"),
        _ => None,
    };
    if let Some(l) = line {
        out.insert("lineStyle".into(), json!(l));
    }
    let point = match r["pointStyle"].as_str() {
        Some("open") => Some("OPEN"),
        Some("cross") => Some("CROSS"),
        _ => None,
    };
    if let Some(p) = point {
        out.insert("pointStyle".into(), json!(p));
    }
    for (from, to, default) in [
        ("width", "lineWidth", 2.5),
        ("pointSize", "pointSize", 9.0),
        ("opacity", "lineOpacity", 1.0),
        ("fillOpacity", "fillOpacity", 0.4),
    ] {
        if let Some(x) = r[from].as_f64().filter(|x| x.is_finite() && *x != default) {
            out.insert(to.into(), json!(desmos_number(x)));
        }
    }
}

/// How a row is named in a warning: its number in the list and its latex.
fn row_name(number: usize, r: &Value) -> String {
    match r["latex"].as_str().filter(|l| !l.is_empty()) {
        Some(l) if l.chars().count() > 40 => {
            format!("Row {number} ({}…)", l.chars().take(40).collect::<String>())
        }
        Some(l) => format!("Row {number} ({l})"),
        None => format!("Row {number}"),
    }
}

/// Function names in latex that Desmos may not know.
fn unknown_functions(latex: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = latex;
    while let Some(at) = rest.find("\\operatorname{") {
        rest = &rest[at + "\\operatorname{".len()..];
        let name: String = rest.chars().take_while(|c| *c != '}').collect();
        if !DESMOS_FUNCTIONS.contains(&name.as_str()) && !names.contains(&name) {
            names.push(name);
        }
    }
    names
}

/// The graph as Desmos's `Calc.setState` takes it, for pasting into
/// desmos.com (desmos.com/3d for a 3D graph), and a warning for each
/// thing that had to be left out because Desmos has no equivalent.
pub fn to_desmos(rows: &Value, vp: &Viewport, space: &Space) -> (Value, Vec<String>) {
    let mut list = Vec::new();
    let mut warnings = Vec::new();
    let mut ticker = None;
    let mut next_id = 0u64;
    let mut id = || {
        next_id += 1;
        json!(next_id.to_string())
    };
    let mut number = 0;
    for r in rows.as_array().map_or(&[][..], Vec::as_slice) {
        if let Some(t) = r.get("ticker").filter(|t| t.is_object()) {
            let mut out = json!({
                "handlerLatex": r["latex"].as_str().unwrap_or(""),
                "minStepLatex": t["step"].as_str().unwrap_or("0"),
                "open": true,
            });
            if t["playing"].as_bool() == Some(true) {
                out["playing"] = json!(true);
            }
            ticker = Some(out);
            continue;
        }
        number += 1;
        let name = row_name(number, r);
        if r["matrix"].is_array() {
            warnings.push(format!(
                "{name} is a matrix, and Desmos's graphing calculator has no matrices, so it was left out."
            ));
            continue;
        }
        let extra: Vec<&str> = r
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(k, v)| !DESMOS_ROW_KEYS.contains(&k.as_str()) && !v.is_null())
            .map(|(k, _)| k.as_str())
            .collect();
        let Some(latex) = r["latex"].as_str() else {
            warnings.push(format!(
                "{name} has no Desmos equivalent, so it was left out."
            ));
            continue;
        };
        for k in &extra {
            warnings.push(format!(
                "{name}: its \"{k}\" setting has no Desmos equivalent, so it was left out."
            ));
        }
        if let Some(note) = r["note"].as_str() {
            list.push(json!({"type": "text", "id": id(), "text": note}));
            continue;
        }
        if let Some(columns) = r["table"].as_array() {
            let table_id = id();
            let columns: Vec<Value> = columns
                .iter()
                .map(|c| {
                    let mut col = serde_json::Map::new();
                    col.insert("id".into(), id());
                    col.insert("latex".into(), json!(c["header"].as_str().unwrap_or("")));
                    let cells: Vec<Value> = c["cells"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|v| json!(v.as_str().unwrap_or("")))
                        .collect();
                    col.insert("values".into(), Value::Array(cells));
                    desmos_style_of(c, &mut col);
                    col.insert("points".into(), json!(c["dots"].as_bool().unwrap_or(true)));
                    col.insert(
                        "lines".into(),
                        json!(c["connect"].as_bool().unwrap_or(false)),
                    );
                    Value::Object(col)
                })
                .collect();
            list.push(json!({"type": "table", "id": table_id, "columns": columns}));
            continue;
        }
        // Desmos keeps no empty rows worth sending.
        if latex.trim().is_empty() {
            continue;
        }
        for f in unknown_functions(latex) {
            warnings.push(format!(
                "{name} uses {f}, which Desmos may not know; Desmos will show an error there."
            ));
        }
        let mut out = serde_json::Map::new();
        out.insert("type".into(), json!("expression"));
        out.insert("id".into(), id());
        out.insert("latex".into(), json!(latex));
        desmos_style_of(r, &mut out);
        // Curves always draw their line in the app, and "connect" only
        // joins points, so only its "on" means anything to Desmos; Desmos's
        // `lines: false` would hide a curve.
        if r["connect"].as_bool() == Some(true) {
            out.insert("lines".into(), json!(true));
        }
        if r["dots"].as_bool() == Some(false) {
            out.insert("points".into(), json!(false));
        }
        let s = &r["slider"];
        if s.is_object() {
            let mut slider = json!({"hardMin": true, "hardMax": true});
            for k in ["min", "max", "step"] {
                if let Some(x) = s[k].as_f64().filter(|x| x.is_finite()) {
                    slider[k] = json!(desmos_number(x));
                }
            }
            slider["loopMode"] = json!(match s["loop"].as_str() {
                Some("forward") => "LOOP_FORWARD",
                Some("once") => "PLAY_ONCE",
                Some("forever") => "PLAY_INDEFINITELY",
                _ => "LOOP_FORWARD_REVERSE",
            });
            if let Some(speed) = s["speed"].as_f64().filter(|x| *x > 0.0 && x.is_finite()) {
                slider["animationPeriod"] = json!((4000.0 / speed).round() as u64);
            }
            out.insert("slider".into(), slider);
        }
        if let Some([a, b]) = r["domain"].as_array().map(Vec::as_slice) {
            if let (Some(a), Some(b)) = (a.as_str(), b.as_str()) {
                out.insert("parametricDomain".into(), json!({"min": a, "max": b}));
            }
        }
        list.push(Value::Object(out));
    }

    let mut expressions = json!({ "list": list });
    if let Some(t) = ticker {
        expressions["ticker"] = t;
    }
    let graph = if space.three_d {
        let b = space
            .view
            .unwrap_or_else(|| BoxView::of(&View3::new(1.0, 1.0)));
        let (sy, cy) = b.yaw.sin_cos();
        let (sp, cp) = b.pitch.sin_cos();
        // The inverse of `desmos_space`: minus the first column points
        // toward the camera, the second runs level across the screen and
        // the third is the screen's up.
        json!({
            "viewport": {
                "xmin": b.center[0] - b.half[0], "xmax": b.center[0] + b.half[0],
                "ymin": b.center[1] - b.half[1], "ymax": b.center[1] + b.half[1],
                "zmin": b.center[2] - b.half[2], "zmax": b.center[2] + b.half[2],
            },
            "worldRotation3D": [
                -cp * cy, sy, -sp * cy,
                -cp * sy, -cy, -sp * sy,
                -sp, 0.0, cp,
            ],
            "product": "graphing-3d",
            "threeDMode": true,
        })
    } else {
        let (w, h) = (
            vp.units_per_px * vp.width_px,
            vp.units_per_px * vp.height_px,
        );
        json!({
            "viewport": {
                "xmin": vp.center[0] - w / 2.0, "xmax": vp.center[0] + w / 2.0,
                "ymin": vp.center[1] - h / 2.0, "ymax": vp.center[1] + h / 2.0,
            },
        })
    };
    let state = json!({
        "version": 11,
        "graph": graph,
        "expressions": expressions,
    });
    (state, warnings)
}

/// Where the session is autosaved: the user's app data folder on Windows.
pub fn autosave_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("graphcalc").join("autosave.json"))
}

/// The autosave: the graph as a file would hold it, plus which file it
/// belongs to and what that file last held.
pub fn autosave_contents(doc: &Document, rows: &Value, vp: &Viewport, space: &Space) -> String {
    let graph: Value = serde_json::from_str(&to_file(rows, vp, space)).unwrap_or(Value::Null);
    json!({
        "path": doc.path,
        "saved": doc.saved,
        "graph": graph,
    })
    .to_string()
}

pub fn write_autosave(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    // Write then rename, so a crash mid-write cannot leave half a file.
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// A restored session: rows for the panel, the view, and the document
/// state to carry on with.
pub struct Restored {
    pub rows: Value,
    pub view: Option<View>,
    pub space: Space,
    pub path: Option<PathBuf>,
    saved: String,
}

impl Restored {
    /// Puts the document back as it was, including unsaved changes.
    pub fn resume(&self, doc: &mut Document) {
        doc.path = self.path.clone();
        doc.saved = self.saved.clone();
        doc.loading = false;
    }
}

pub fn read_autosave(text: &str) -> Option<Restored> {
    let a: Value = serde_json::from_str(text).ok()?;
    let graph = a["graph"].to_string();
    let (rows, view) = from_file(&graph).ok()?;
    Some(Restored {
        rows,
        view,
        space: space_of(&graph),
        path: a["path"].as_str().map(PathBuf::from),
        saved: a["saved"].as_str().unwrap_or("").to_owned(),
    })
}

/// Adds the graph extension when a typed name has none.
pub fn with_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_some() {
        path
    } else {
        path.with_extension(EXTENSION)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Value {
        json!([
            {"id": 4, "latex": "a=2", "color": "#2d70b3", "slider": {"min": -10, "max": 10, "step": null}},
            {"id": 5, "latex": "", "table": [{"header": "x_1", "cells": ["1", ""]}]},
        ])
    }

    #[test]
    fn files_round_trip_rows_and_view() {
        let mut vp = Viewport::new(1000.0, 600.0);
        vp.center = [3.0, -1.0];
        vp.units_per_px = 0.05;
        let text = to_file(&rows(), &vp, &Space::default());
        let (back, view) = from_file(&text).unwrap();
        assert_eq!(space_of(&text), Space::default());
        assert_eq!(back, strip_ids(&rows()));
        assert!(back[0].get("id").is_none());

        // Opened in a wider window the same width of paper shows.
        let mut other = Viewport::new(2000.0, 900.0);
        view.unwrap().apply(&mut other);
        assert_eq!(other.center, [3.0, -1.0]);
        assert!((other.units_per_px - 0.025).abs() < 1e-15);
    }

    #[test]
    fn bad_files_say_why() {
        assert!(from_file("not json")
            .unwrap_err()
            .contains("isn't a graph file"));
        assert!(from_file(r#"{"format":"other"}"#).is_err());
        let newer = r#"{"format":"graphcalc","version":99,"rows":[]}"#;
        assert!(from_file(newer).unwrap_err().contains("newer version"));
        let no_view = r#"{"format":"graphcalc","version":1,"rows":[]}"#;
        assert_eq!(from_file(no_view).unwrap(), (json!([]), None));
    }

    #[test]
    fn documents_track_changes_and_titles() {
        let mut doc = Document::default();
        doc.loading(Some(PathBuf::from("C:/graphs/parabola.gcalc")));
        doc.rows_changed(&rows());
        assert!(!doc.dirty(), "a freshly opened graph is unchanged");
        assert_eq!(doc.title(), "parabola - Graphing Calculator");

        // New ids alone are not a change.
        let mut renumbered = rows();
        renumbered[0]["id"] = json!(40);
        doc.rows_changed(&renumbered);
        assert!(!doc.dirty());

        let mut edited = rows();
        edited[0]["latex"] = json!("a=3");
        doc.rows_changed(&edited);
        assert!(doc.dirty());
        assert_eq!(doc.title(), "*parabola - Graphing Calculator");
        doc.saved_to(PathBuf::from("C:/graphs/parabola2.gcalc"));
        assert!(!doc.dirty());
    }

    #[test]
    fn autosave_brings_back_unsaved_work() {
        let vp = Viewport::new(800.0, 600.0);
        let mut doc = Document::default();
        doc.loading(Some(PathBuf::from("g.gcalc")));
        doc.rows_changed(&rows());
        let mut edited = rows();
        edited[0]["latex"] = json!("a=7");
        doc.rows_changed(&edited);
        let mut view3 = View3::new(800.0, 600.0);
        view3.turn_px(50.0, 20.0);
        view3.zoom(2.0);
        let space = Space {
            three_d: true,
            view: Some(BoxView::of(&view3)),
        };
        let text = autosave_contents(&doc, &edited, &vp, &space);

        let restored = read_autosave(&text).unwrap();
        assert_eq!(restored.rows[0]["latex"], "a=7");
        assert_eq!(restored.space, space, "the mode and the box come back");
        assert_eq!(restored.path.as_deref(), Some(Path::new("g.gcalc")));
        let mut again = Document::default();
        restored.resume(&mut again);
        again.rows_changed(&restored.rows);
        assert!(again.dirty(), "the unsaved edit is still unsaved");

        // Switching to 3D and back is a change, then not one.
        let mut doc = Document::default();
        doc.loading(None);
        doc.rows_changed(&rows());
        doc.mode_changed(true);
        assert!(doc.dirty());
        doc.mode_changed(false);
        assert!(!doc.dirty());
        assert_eq!(with_extension(PathBuf::from("a")), PathBuf::from("a.gcalc"));
    }

    #[test]
    fn desmos_links_and_pastes_are_recognised() {
        let url = Some("https://www.desmos.com/calculator/iotrkvyz1u".to_string());
        assert_eq!(
            desmos_link("https://www.desmos.com/calculator/iotrkvyz1u"),
            url
        );
        assert_eq!(
            desmos_link(" desmos.com/calculator/iotrkvyz1u?lang=en\n"),
            url
        );
        assert_eq!(desmos_link("www.desmos.com/calculator/iotrkvyz1u"), url);
        assert_eq!(desmos_link("https://www.desmos.com/calculator"), None);
        assert_eq!(desmos_link("y=x see desmos.com/calculator/abc"), None);
        assert_eq!(
            desmos_link("https://evil.example/desmos.com/calculator/abc"),
            None
        );
        assert_eq!(
            desmos_link("https://www.desmos.com/3d/abc123"),
            Some("https://www.desmos.com/3d/abc123".to_string())
        );
        let state = r#"{"version":11,"expressions":{"list":[{"type":"expression","id":"1","latex":"y=x"}]}}"#;
        assert!(desmos_state(state).is_some());
        let reply = format!(r#"{{"title":"t","hash":"abc","state":{state}}}"#);
        let (rows, _, imported) = open_any(&reply).unwrap();
        assert!(imported);
        assert_eq!(rows[0]["latex"], "y=x");
        assert!(desmos_state("y=x").is_none());
    }

    #[test]
    fn desmos_3d_states_open_in_3d() {
        let state = json!({
            "version": 11,
            "graph": {
                "threeDMode": true,
                "viewport": {"xmin": -5, "xmax": 5, "ymin": -5, "ymax": 5, "zmin": 0, "zmax": 20},
                "worldRotation3D": [0.1423, -0.9852, 0.0956, 0.8177, 0.1715, 0.5496, -0.5578, 0.0, 0.83],
            },
            "expressions": {"list": [{"type": "expression", "id": "1", "latex": "z=x^2+y^2"}]}
        })
        .to_string();
        let space = space_of(&state);
        assert!(space.three_d);
        let view = space.view.unwrap();
        assert_eq!(
            (view.center, view.half),
            ([0.0, 0.0, 10.0], [5.0, 5.0, 10.0])
        );
        // The camera Desmos saved: from the -y side, about 34° up.
        assert!((view.pitch - 0.5917).abs() < 1e-3, "{}", view.pitch);
        assert!((view.yaw + 1.7431).abs() < 1e-3, "{}", view.yaw);
        assert_eq!(open_any(&state).unwrap().0[0]["latex"], "z=x^2+y^2");
        // A 2D graph stays 2D.
        let flat = r#"{"graph":{"viewport":{"xmin":-1,"xmax":1,"ymin":-1,"ymax":1}},"expressions":{"list":[]}}"#;
        assert!(!space_of(flat).three_d);
    }

    #[test]
    fn desmos_states_open_as_graphs() {
        let state = json!({
            "version": 11,
            "graph": {"viewport": {"xmin": -10, "xmax": 30, "ymin": -5, "ymax": 15}},
            "expressions": {
                "list": [
                    {"type": "folder", "id": "1", "title": "stuff"},
                    {"type": "expression", "id": "2", "color": "#c74440", "latex": "a=1",
                     "slider": {"hardMin": true, "min": "0", "step": "0.01",
                                "loopMode": "PLAY_ONCE", "animationPeriod": 40000}},
                    {"type": "expression", "id": "3", "color": "#2d70b3", "latex": "(t,t^2)",
                     "lineStyle": "DASHED", "lineWidth": "5", "parametricDomain": {"min": "-1", "max": "\\pi"}},
                    {"type": "text", "id": "4", "text": "a note"},
                    {"type": "table", "id": "5", "columns": [
                        {"latex": "x_1", "values": ["1", "2"], "color": "#000000"},
                        {"latex": "y_1", "values": ["3", ""], "points": false, "lines": true}
                    ]}
                ],
                "ticker": {"handlerLatex": "a\\to a+1", "minStepLatex": "50", "open": true}
            }
        });
        let (rows, view, imported) = open_any(&state.to_string()).unwrap();
        assert!(imported);
        assert_eq!(
            view,
            Some(View {
                center: [10.0, 5.0],
                width: 40.0
            })
        );
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 5, "the folder is left out");
        assert_eq!(rows[3], json!({"latex": "", "note": "a note"}));
        assert_eq!(rows[0]["ticker"]["step"], "50");
        assert_eq!(rows[0]["latex"], "a\\to a+1");
        assert_eq!(
            rows[1]["slider"],
            json!({"min": 0.0, "max": 10.0, "step": 0.01, "loop": "once", "speed": 0.1})
        );
        assert_eq!(rows[1]["color"], "#c74440");
        assert_eq!(rows[2]["style"], "dashed");
        assert_eq!(rows[2]["width"], 5.0);
        assert_eq!(rows[2]["domain"], json!(["-1", "\\pi"]));
        assert_eq!(rows[4]["table"][1]["cells"], json!(["3", ""]));
        assert_eq!(rows[4]["table"][1]["connect"], true);

        // The app's own files open as before.
        let own = r#"{"format":"graphcalc","version":1,"rows":[]}"#;
        assert_eq!(open_any(own).unwrap(), (json!([]), None, false));
    }

    #[test]
    fn graphs_export_to_desmos_and_back() {
        let rows = json!([
            {"id": 9, "latex": "a\\to a+1", "hidden": true, "ticker": {"step": "50", "playing": false}},
            {"id": 1, "latex": "a=1", "color": "#c74440", "hidden": false, "style": "solid",
             "width": 2.5, "opacity": 1, "fillOpacity": 0.4, "pointStyle": "filled", "pointSize": 9,
             "dots": true, "connect": false, "domain": null,
             "slider": {"min": 0.0, "max": 10.0, "step": 0.01, "loop": "once", "speed": 0.1}},
            {"id": 2, "latex": "(t,t^2)", "color": "#2d70b3", "style": "dashed", "width": 5,
             "connect": false, "domain": ["-1", "\\pi"]},
            {"id": 3, "latex": "", "note": "a note"},
            {"id": 4, "latex": "", "table": [
                {"header": "x_1", "cells": ["1", "2"], "color": "#000000", "dots": true, "connect": false},
                {"header": "y_1", "cells": ["3", ""], "dots": false, "connect": true}
            ]},
            {"id": 5, "latex": ""},
            {"id": 6, "latex": "y=\\operatorname{zeta}(x)"},
            {"id": 7, "latex": "y'=y", "slopeField": true},
            {"id": 8, "matrix": [[1, 2], [3, 4]]}
        ]);
        let mut vp = Viewport::new(400.0, 200.0);
        View {
            center: [10.0, 5.0],
            width: 40.0,
        }
        .apply(&mut vp);
        let (state, warnings) = to_desmos(&rows, &vp, &Space::default());
        assert_eq!(
            state["graph"]["viewport"],
            json!({"xmin": -10.0, "xmax": 30.0, "ymin": -5.0, "ymax": 15.0})
        );
        let list = state["expressions"]["list"].as_array().unwrap();
        assert_eq!(list.len(), 6, "the empty row and the matrix are left out");
        // Defaults are left for Desmos to fill in, and a curve's lines stay on.
        assert_eq!(
            list[0],
            json!({"type": "expression", "id": "1", "latex": "a=1", "color": "#c74440",
                   "slider": {"hardMin": true, "hardMax": true, "min": "0", "max": "10",
                              "step": "0.01", "loopMode": "PLAY_ONCE", "animationPeriod": 40000}})
        );
        assert_eq!(list[1]["lineWidth"], "5");
        assert_eq!(list[5]["latex"], "y'=y");
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        assert!(warnings[0].contains("zeta"));
        assert!(warnings[1].contains("slopeField"));
        assert!(warnings[2].starts_with("Row 8 is a matrix"));

        // Importing the export gives the graph back.
        let (back, view, _) = open_any(&state.to_string()).unwrap();
        assert_eq!(
            view,
            Some(View {
                center: [10.0, 5.0],
                width: 40.0
            })
        );
        assert_eq!(back[0]["ticker"]["step"], "50");
        assert_eq!(back[0]["latex"], "a\\to a+1");
        assert_eq!(back[1]["slider"], rows[1]["slider"]);
        assert_eq!(back[2]["style"], "dashed");
        assert_eq!(back[2]["domain"], json!(["-1", "\\pi"]));
        assert_eq!(back[3]["note"], "a note");
        assert_eq!(back[4]["table"][1]["cells"], json!(["3", ""]));
        assert_eq!(back[4]["table"][1]["connect"], true);
        assert_eq!(back[4]["table"][1]["dots"], false);
        assert!(!space_of(&state.to_string()).three_d);
    }

    #[test]
    fn graphs_export_to_desmos_3d_at_their_camera() {
        let view = BoxView {
            center: [1.0, 2.0, 3.0],
            half: [4.0, 5.0, 6.0],
            yaw: -1.7431,
            pitch: 0.5917,
        };
        let space = Space {
            three_d: true,
            view: Some(view),
        };
        let rows = json!([{"latex": "z=x^2+y^2"}]);
        let (state, warnings) = to_desmos(&rows, &Viewport::new(100.0, 100.0), &space);
        assert!(warnings.is_empty());
        assert_eq!(state["graph"]["threeDMode"], true);
        assert_eq!(state["graph"]["viewport"]["zmax"], 9.0);
        // Close to the rotation Desmos saved for Ezra's Riemann sum.
        let m: Vec<f64> = state["graph"]["worldRotation3D"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let desmos = [
            0.1423, -0.9852, 0.0956, 0.8177, 0.1715, 0.5496, -0.5578, 0.0, 0.83,
        ];
        for (a, b) in m.iter().zip(desmos) {
            assert!((a - b).abs() < 1e-3, "{m:?}");
        }
        let back = space_of(&state.to_string());
        assert!(back.three_d);
        let b = back.view.unwrap();
        assert_eq!((b.center, b.half), (view.center, view.half));
        assert!((b.yaw - view.yaw).abs() < 1e-9 && (b.pitch - view.pitch).abs() < 1e-9);
    }
}
