//! Math documents: the files behind the Document view. Each document is one
//! `.gcdoc` file of JSON (title and lines), kept in a library folder that the
//! document sidebar lists. The panel builds the JSON; this side checks it,
//! names files and reads and writes them.

#![cfg_attr(not(windows), allow(dead_code))]

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const EXTENSION: &str = "gcdoc";
const FORMAT: &str = "graphcalc-document";
const VERSION: u64 = 1;
/// The library folder's name inside the person's Documents folder.
pub const LIBRARY_NAME: &str = "Graph Calculator";

/// Checks a document the panel sent (or a file holds) and returns it in the
/// form files are written in.
pub fn normalize(doc: &Value) -> Result<Value, String> {
    let lines = doc["lines"]
        .as_array()
        .ok_or("this is not a math document (it has no lines)")?;
    if let Some(format) = doc["format"].as_str() {
        if format != FORMAT {
            return Err(format!("this is a {format} file, not a math document"));
        }
    }
    let version = doc["version"].as_u64().unwrap_or(VERSION);
    if version > VERSION {
        return Err("this document was saved by a newer version of the app".into());
    }
    let mut out = Vec::with_capacity(lines.len());
    for line in lines {
        let kind = line["type"].as_str().unwrap_or("math");
        let text = |key: &str| line[key].as_str().unwrap_or_default().to_owned();
        out.push(match kind {
            "text" => json!({ "type": "text", "text": text("text") }),
            "heading" => {
                let level = line["level"].as_u64().unwrap_or(1).clamp(1, 3);
                json!({ "type": "heading", "level": level, "text": text("text") })
            }
            _ => {
                let mut math = json!({ "type": "math", "latex": text("latex") });
                // A step continues the group of steps above it; a group's
                // first line keeps an id that references point at.
                if line["step"] == true {
                    math["step"] = Value::Bool(true);
                }
                // A live line computes; every other line is only typeset.
                if line["live"] == true {
                    math["live"] = Value::Bool(true);
                }
                if let Some(id) = line["id"].as_str().filter(|id| is_id(id)) {
                    math["id"] = Value::String(id.to_owned());
                }
                math
            }
        });
    }
    Ok(json!({
        "format": FORMAT,
        "version": VERSION,
        "title": doc["title"].as_str().unwrap_or_default(),
        "lines": out,
    }))
}

/// Line ids are short words, as MathQuill's embed data allows.
fn is_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The person's own typing shortcuts, shared by every document.
pub fn shortcuts_path() -> Option<PathBuf> {
    crate::files::autosave_path().map(|p| p.with_file_name("shortcuts.json"))
}

/// The saved shortcuts, or null before any were changed (the panel then
/// uses its built-in set).
pub fn read_shortcuts() -> Value {
    shortcuts_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v.is_array())
        .unwrap_or(Value::Null)
}

pub fn write_shortcuts(list: &Value) -> Result<(), String> {
    if !list.is_array() {
        return Err("shortcuts must be a list".into());
    }
    let path = shortcuts_path().ok_or("no settings folder")?;
    let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    write(&path, &text)
}

/// The text of a document file.
pub fn to_file(doc: &Value) -> Result<String, String> {
    let doc = normalize(doc)?;
    serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
}

/// Reads a document file's text.
pub fn from_file(text: &str) -> Result<Value, String> {
    let value: Value = serde_json::from_str(text).map_err(|_| "this is not a math document")?;
    if value["format"].as_str() != Some(FORMAT) {
        return Err("this is not a math document".into());
    }
    normalize(&value)
}

/// A file name for a title: characters Windows refuses become spaces, and an
/// empty title is "Untitled".
pub fn file_stem(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let mut stem = words.join(" ");
    // Windows drops trailing dots, and names like CON are reserved.
    while stem.ends_with('.') {
        stem.pop();
    }
    let reserved = ["CON", "PRN", "AUX", "NUL"];
    let upper = stem.to_ascii_uppercase();
    if reserved.contains(&upper.as_str())
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && upper.as_bytes()[3].is_ascii_digit())
    {
        stem.push('_');
    }
    let stem: String = stem.chars().take(80).collect();
    if stem.trim().is_empty() {
        "Untitled".into()
    } else {
        stem.trim().to_owned()
    }
}

/// A path in `dir` for `title` that no file uses yet: "Notes.gcdoc", then
/// "Notes (2).gcdoc" and so on. `keep` is a path that may be reused (the
/// document's own file, when renaming).
pub fn free_path(dir: &Path, title: &str, keep: Option<&Path>) -> PathBuf {
    let stem = file_stem(title);
    for n in 1.. {
        let name = if n == 1 {
            format!("{stem}.{EXTENSION}")
        } else {
            format!("{stem} ({n}).{EXTENSION}")
        };
        let path = dir.join(name);
        if Some(path.as_path()) == keep || !path.exists() {
            return path;
        }
    }
    unreachable!()
}

/// Writes a file through a temporary, so a crash cannot leave half of it.
pub fn write(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension(format!("{EXTENSION}.tmp"));
    std::fs::write(&tmp, contents).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Every document in the library: those in the folder itself and one level
/// of subfolders (one per class, say), newest first.
pub fn list(library: &Path) -> Vec<Value> {
    let mut items = Vec::new();
    collect(library, "", &mut items);
    if let Ok(entries) = std::fs::read_dir(library) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.starts_with('.') {
                    collect(&path, &name, &mut items);
                }
            }
        }
    }
    items.sort_by_key(|(modified, _): &(u64, Value)| std::cmp::Reverse(*modified));
    items.into_iter().map(|(_, v)| v).collect()
}

fn collect(dir: &Path, group: &str, items: &mut Vec<(u64, Value)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(EXTENSION) || !path.is_file() {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        items.push((
            modified,
            json!({ "path": path, "name": name, "group": group, "modified": modified }),
        ));
    }
}

/// Which view was showing and which document was open, so the next start
/// carries on from there.
pub fn state_path() -> Option<PathBuf> {
    crate::files::autosave_path().map(|p| p.with_file_name("documents.json"))
}

pub struct State {
    pub doc_view: bool,
    pub path: Option<PathBuf>,
    pub library: Option<PathBuf>,
    /// The PDF export choices last used (page size, header, ...).
    pub pdf: Value,
}

pub fn read_state() -> State {
    let value: Value = state_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(Value::Null);
    State {
        doc_view: value["view"] == "doc",
        path: value["path"].as_str().map(PathBuf::from),
        library: value["library"].as_str().map(PathBuf::from),
        pdf: value["pdf"].clone(),
    }
}

pub fn write_state(state: &State) {
    let Some(path) = state_path() else { return };
    let value = json!({
        "view": if state.doc_view { "doc" } else { "graph" },
        "path": state.path,
        "library": state.library,
        "pdf": state.pdf,
    });
    // Losing this only means starting in the graph view.
    let _ = write(&path, &value.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("graphcalc-docs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn documents_round_trip() {
        let doc = json!({
            "title": "Problem set 4",
            "lines": [
                { "type": "heading", "level": 1, "text": "Problem 1" },
                { "type": "text", "text": "Evaluate $\\int_0^1 x^2dx$." },
                { "type": "math", "latex": "I=\\frac{1}{3}", "id": "g1", "live": true },
                { "type": "math", "latex": "=1/3", "step": true, "id": "bad id!" },
                { "type": "mystery", "latex": "x" }
            ]
        });
        let text = to_file(&doc).unwrap();
        let back = from_file(&text).unwrap();
        assert_eq!(back["title"], "Problem set 4");
        assert_eq!(back["lines"][1]["text"], "Evaluate $\\int_0^1 x^2dx$.");
        assert_eq!(back["lines"][2]["latex"], "I=\\frac{1}{3}");
        assert_eq!(back["lines"][2]["id"], "g1");
        assert_eq!(back["lines"][3]["step"], true);
        assert!(back["lines"][3]["id"].is_null());
        assert!(back["lines"][2]["step"].is_null());
        assert_eq!(back["lines"][2]["live"], true);
        assert!(back["lines"][3]["live"].is_null());
        // Unknown kinds read as math lines.
        assert_eq!(back["lines"][4]["type"], "math");
    }

    #[test]
    fn other_files_are_refused() {
        assert!(from_file("{\"format\":\"graphcalc\",\"rows\":[]}").is_err());
        assert!(from_file("not json").is_err());
    }

    #[test]
    fn titles_become_safe_file_names() {
        assert_eq!(file_stem("Calc 2: HW 3/4"), "Calc 2 HW 3 4");
        assert_eq!(file_stem("   "), "Untitled");
        assert_eq!(file_stem("con"), "con_");
        assert_eq!(file_stem("notes..."), "notes");
    }

    #[test]
    fn free_paths_do_not_overwrite() {
        let dir = temp_dir("free");
        let first = free_path(&dir, "Notes", None);
        assert!(first.ends_with("Notes.gcdoc"));
        std::fs::write(&first, "x").unwrap();
        let second = free_path(&dir, "Notes", None);
        assert!(second.ends_with("Notes (2).gcdoc"));
        // A document may keep its own name when renamed to the same title.
        assert_eq!(free_path(&dir, "Notes", Some(&first)), first);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn library_lists_subfolders_newest_first() {
        let dir = temp_dir("list");
        let doc = to_file(&json!({ "lines": [] })).unwrap();
        write(&dir.join("a.gcdoc"), &doc).unwrap();
        std::fs::create_dir_all(dir.join("Calc 2")).unwrap();
        write(&dir.join("Calc 2").join("b.gcdoc"), &doc).unwrap();
        std::fs::write(dir.join("ignored.txt"), "x").unwrap();
        let items = list(&dir);
        assert_eq!(items.len(), 2);
        let names: Vec<&str> = items.iter().map(|i| i["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"a") && names.contains(&"b"));
        let b = items.iter().find(|i| i["name"] == "b").unwrap();
        assert_eq!(b["group"], "Calc 2");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
