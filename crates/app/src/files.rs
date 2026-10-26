//! Saving and opening graphs. A graph file is JSON holding the panel's rows
//! (as the panel sends them, minus their ids) and the view. The session is
//! also autosaved, so closing the window never loses work and the next
//! launch picks up where the last one left off.

// Only the Windows window saves and opens outside tests.
#![cfg_attr(not(windows), allow(dead_code))]

use graph_core::Viewport;
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
    /// The rows as the panel last sent them.
    current: String,
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
        self.current = strip_ids(rows).to_string();
        if self.loading {
            self.loading = false;
            self.saved = self.current.clone();
        }
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
pub fn to_file(rows: &Value, vp: &Viewport) -> String {
    let file = json!({
        "format": FORMAT,
        "version": VERSION,
        "viewport": {
            "center": vp.center,
            "width": vp.units_per_px * vp.width_px,
        },
        "rows": strip_ids(rows),
    });
    serde_json::to_string_pretty(&file).unwrap_or_default()
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
    if let Ok(file) = serde_json::from_str::<Value>(text) {
        if file["expressions"]["list"].is_array() {
            let (rows, view) = from_desmos(&file);
            return Ok((rows, view, true));
        }
    }
    from_file(text).map(|(rows, view)| (rows, view, false))
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

/// Where the session is autosaved: the user's app data folder on Windows.
pub fn autosave_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("graphcalc").join("autosave.json"))
}

/// The autosave: the graph as a file would hold it, plus which file it
/// belongs to and what that file last held.
pub fn autosave_contents(doc: &Document, rows: &Value, vp: &Viewport) -> String {
    let graph: Value = serde_json::from_str(&to_file(rows, vp)).unwrap_or(Value::Null);
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
    let (rows, view) = from_file(&a["graph"].to_string()).ok()?;
    Some(Restored {
        rows,
        view,
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
        let text = to_file(&rows(), &vp);
        let (back, view) = from_file(&text).unwrap();
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
        let text = autosave_contents(&doc, &edited, &vp);

        let restored = read_autosave(&text).unwrap();
        assert_eq!(restored.rows[0]["latex"], "a=7");
        assert_eq!(restored.path.as_deref(), Some(Path::new("g.gcalc")));
        let mut again = Document::default();
        restored.resume(&mut again);
        again.rows_changed(&restored.rows);
        assert!(again.dirty(), "the unsaved edit is still unsaved");
        assert_eq!(with_extension(PathBuf::from("a")), PathBuf::from("a.gcalc"));
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
        assert_eq!(rows.len(), 4, "the folder and the note are left out");
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
        assert_eq!(rows[3]["table"][1]["cells"], json!(["3", ""]));
        assert_eq!(rows[3]["table"][1]["connect"], true);

        // The app's own files open as before.
        let own = r#"{"format":"graphcalc","version":1,"rows":[]}"#;
        assert_eq!(open_any(own).unwrap(), (json!([]), None, false));
    }
}
