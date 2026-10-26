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
}
