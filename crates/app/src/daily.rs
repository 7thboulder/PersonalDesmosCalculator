//! The daily problems archive. dailyintegral.com posts an integral, a
//! derivative and a differential equation at each level every day, each
//! with two hints. The app loads the site's own pages in a hidden browser
//! panel (`daily_capture.js` watches what they load) and keeps every
//! problem it sees in `daily.json` beside the autosave, so old days stay
//! after the site moves on (it charges for its archive).

#![cfg_attr(not(windows), allow(dead_code))]

use serde_json::{json, Value};
use std::path::PathBuf;

pub const SITE: &str = "https://dailyintegral.com";

/// The page script that reports what the site's pages fetch.
pub const CAPTURE_SCRIPT: &str = include_str!("../ui/daily_capture.js");

/// One of the day's problems: its kind, its level as the site names it,
/// and the page that shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Page {
    pub kind: &'static str,
    pub level: &'static str,
    pub path: &'static str,
}

/// Every free problem of the day, in the order the archive lists them.
pub const PAGES: [Page; 10] = [
    Page {
        kind: "integral",
        level: "BEGINNER",
        path: "/play/beginner",
    },
    Page {
        kind: "integral",
        level: "EASY",
        path: "/play/easy",
    },
    Page {
        kind: "integral",
        level: "MEDIUM",
        path: "/play/medium",
    },
    Page {
        kind: "integral",
        level: "HARD",
        path: "/play/hard",
    },
    Page {
        kind: "derivative",
        level: "EASY",
        path: "/play/derivatives/easy",
    },
    Page {
        kind: "derivative",
        level: "MEDIUM",
        path: "/play/derivatives/medium",
    },
    Page {
        kind: "derivative",
        level: "HARD",
        path: "/play/derivatives/hard",
    },
    Page {
        kind: "differential",
        level: "EASY",
        path: "/play/differential/easy",
    },
    Page {
        kind: "differential",
        level: "MEDIUM",
        path: "/play/differential/medium",
    },
    Page {
        kind: "differential",
        level: "HARD",
        path: "/play/differential/hard",
    },
];

/// Whether the hidden panel may go to `url`: only the site's own pages.
pub fn allowed_url(url: &str) -> bool {
    url == SITE || url.starts_with(&format!("{SITE}/")) || url == "about:blank"
}

fn order(kind: &str, level: &str) -> usize {
    PAGES
        .iter()
        .position(|p| p.kind == kind && p.level == level)
        .unwrap_or(PAGES.len())
}

/// Something one of the site's pages loaded.
#[derive(Debug, PartialEq)]
pub enum Captured {
    Puzzle {
        day: u64,
        kind: String,
        level: String,
        title: String,
        slug: String,
        latex: String,
    },
    Hint {
        day: u64,
        kind: String,
        level: String,
        index: usize,
        text: String,
    },
}

/// Reads a message from `daily_capture.js`. Anything else, or anything
/// that doesn't look like the site's answers, is None.
pub fn parse_capture(message: &str) -> Option<Captured> {
    let msg: Value = serde_json::from_str(message).ok()?;
    if msg["type"] != "dailyCaptured" {
        return None;
    }
    let request: Value = serde_json::from_str(msg["request"].as_str()?).ok()?;
    let response: Value = serde_json::from_str(msg["response"].as_str()?).ok()?;
    let kind = request["type"].as_str()?;
    if !matches!(kind, "integral" | "derivative" | "differential") {
        return None;
    }
    let day = request["day"].as_u64().filter(|d| *d >= 1)?;
    let level = request["difficulty"].as_str()?.to_ascii_uppercase();
    match msg["fn"].as_str()? {
        "get-daily-puzzle" => {
            let p = &response["puzzle"];
            let latex = p["latex"].as_str()?.to_owned();
            Some(Captured::Puzzle {
                day: p["day"].as_u64().unwrap_or(day),
                kind: kind.to_owned(),
                level: p["difficulty"]
                    .as_str()
                    .map(str::to_ascii_uppercase)
                    .unwrap_or(level),
                title: p["title"].as_str().unwrap_or_default().to_owned(),
                slug: p["slug"].as_str().unwrap_or_default().to_owned(),
                latex,
            })
        }
        "get-puzzle-hint" => {
            let index = request["index"].as_u64().filter(|i| *i < 2)? as usize;
            let text = response["hint"].as_str()?.trim().to_owned();
            if text.is_empty() {
                return None;
            }
            Some(Captured::Hint {
                day,
                kind: kind.to_owned(),
                level,
                index,
                text,
            })
        }
        _ => None,
    }
}

/// The site's day number as a date: day 1 was 16 September 2025 (Sydney).
pub fn day_date(day: u64) -> String {
    // Days since 1970-01-01 for 2025-09-16, then back to a civil date.
    let start = days_from_civil(2025, 9, 16);
    let (y, m, d) = civil_from_days(start + day as i64 - 1);
    format!("{y:04}-{m:02}-{d:02}")
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// The archive: `{ fetchOnOpen, lastFetch, days: [{ day, date, problems }] }`,
/// newest day first. Fetching on open is off until it is turned on.
pub fn empty() -> Value {
    json!({ "fetchOnOpen": false, "days": [] })
}

pub fn path() -> Option<PathBuf> {
    crate::files::autosave_path().map(|p| p.with_file_name("daily.json"))
}

pub fn read() -> Value {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|v| tidy(&v))
        .unwrap_or_else(empty)
}

pub fn write(archive: &Value) -> Result<(), String> {
    let path = path().ok_or("no settings folder")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let text = serde_json::to_string_pretty(archive).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

/// A file's archive with anything malformed left out.
fn tidy(v: &Value) -> Value {
    let mut out = empty();
    out["fetchOnOpen"] = Value::Bool(v["fetchOnOpen"] == true);
    if let Some(t) = v["lastFetch"].as_str() {
        out["lastFetch"] = Value::String(t.to_owned());
    }
    let days: Vec<Value> = v["days"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| d["day"].as_u64().is_some() && d["problems"].is_array())
        .cloned()
        .collect();
    out["days"] = Value::Array(days);
    out
}

/// The day's entry, made if it isn't there yet, keeping days newest first.
fn day_mut(archive: &mut Value, day: u64) -> &mut Value {
    if !archive["days"].is_array() {
        archive["days"] = json!([]);
    }
    let days = archive["days"].as_array_mut().expect("days is a list");
    let at = match days.iter().position(|d| d["day"].as_u64() == Some(day)) {
        Some(at) => at,
        None => {
            let at = days
                .iter()
                .position(|d| d["day"].as_u64().unwrap_or(0) < day)
                .unwrap_or(days.len());
            days.insert(
                at,
                json!({ "day": day, "date": day_date(day), "problems": [] }),
            );
            at
        }
    };
    &mut days[at]
}

/// The problem's entry within its day, made if it isn't there yet, keeping
/// the day's problems in the order of `PAGES`.
fn problem_mut<'a>(archive: &'a mut Value, day: u64, kind: &str, level: &str) -> &'a mut Value {
    let entry = day_mut(archive, day);
    let problems = entry["problems"]
        .as_array_mut()
        .expect("problems is a list");
    let at = match problems
        .iter()
        .position(|p| p["kind"] == kind && p["level"] == level)
    {
        Some(at) => at,
        None => {
            let rank = order(kind, level);
            let at = problems
                .iter()
                .position(|p| {
                    order(
                        p["kind"].as_str().unwrap_or_default(),
                        p["level"].as_str().unwrap_or_default(),
                    ) > rank
                })
                .unwrap_or(problems.len());
            problems.insert(
                at,
                json!({ "kind": kind, "level": level, "title": "", "latex": "", "hints": [null, null] }),
            );
            at
        }
    };
    &mut problems[at]
}

/// Keeps what a page loaded. Returns whether the archive changed.
pub fn add(archive: &mut Value, captured: &Captured) -> bool {
    match captured {
        Captured::Puzzle {
            day,
            kind,
            level,
            title,
            slug,
            latex,
        } => {
            let p = problem_mut(archive, *day, kind, level);
            let before = p.clone();
            p["title"] = Value::String(title.clone());
            p["slug"] = Value::String(slug.clone());
            p["latex"] = Value::String(latex.clone());
            *p != before
        }
        Captured::Hint {
            day,
            kind,
            level,
            index,
            text,
        } => {
            let p = problem_mut(archive, *day, kind, level);
            if !p["hints"].is_array() {
                p["hints"] = json!([null, null]);
            }
            let hints = p["hints"].as_array_mut().expect("hints is a list");
            while hints.len() <= *index {
                hints.push(Value::Null);
            }
            if hints[*index].as_str() == Some(text) {
                return false;
            }
            hints[*index] = Value::String(text.clone());
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(f: &str, request: Value, response: Value) -> String {
        json!({
            "type": "dailyCaptured",
            "fn": f,
            "request": request.to_string(),
            "response": response.to_string(),
        })
        .to_string()
    }

    #[test]
    fn days_count_from_the_sites_first_day() {
        assert_eq!(day_date(1), "2025-09-16");
        assert_eq!(day_date(436), "2026-11-25");
        assert_eq!(day_date(107), "2025-12-31");
        assert_eq!(day_date(108), "2026-01-01");
    }

    #[test]
    fn reads_a_puzzle_and_its_hints() {
        let puzzle = message(
            "get-daily-puzzle",
            json!({"type": "derivative", "day": 436, "difficulty": "EASY"}),
            json!({"puzzle": {"day": 436, "difficulty": "EASY", "slug": "s", "title": "T",
                "latex": "\\text{Find } f'(1).", "answerSalt": "x", "answerHashesV2": ["a"]}}),
        );
        let hint = message(
            "get-puzzle-hint",
            json!({"type": "derivative", "day": 436, "difficulty": "EASY", "index": 1}),
            json!({"hint": "Use the quotient rule."}),
        );
        let mut archive = empty();
        assert!(add(&mut archive, &parse_capture(&hint).unwrap()));
        assert!(add(&mut archive, &parse_capture(&puzzle).unwrap()));
        assert!(!add(&mut archive, &parse_capture(&puzzle).unwrap()));
        let day = &archive["days"][0];
        assert_eq!(day["date"], "2026-11-25");
        let p = &day["problems"][0];
        assert_eq!(p["title"], "T");
        assert_eq!(p["latex"], "\\text{Find } f'(1).");
        assert_eq!(p["hints"], json!([null, "Use the quotient rule."]));
        // The answer checks are not kept.
        assert!(p.get("answerHashesV2").is_none());
    }

    #[test]
    fn ignores_other_messages() {
        assert_eq!(parse_capture("{}"), None);
        assert_eq!(parse_capture("not json"), None);
        let logic = message(
            "get-daily-puzzle",
            json!({"type": "logic", "day": 4}),
            json!({"puzzle": {"latex": "x"}}),
        );
        assert_eq!(parse_capture(&logic), None);
        let empty_hint = message(
            "get-puzzle-hint",
            json!({"type": "integral", "day": 4, "difficulty": "EASY", "index": 0}),
            json!({"hint": ""}),
        );
        assert_eq!(parse_capture(&empty_hint), None);
    }

    #[test]
    fn keeps_days_newest_first_and_problems_in_page_order() {
        let mut archive = empty();
        let puzzle = |day, kind: &str, level: &str| Captured::Puzzle {
            day,
            kind: kind.into(),
            level: level.into(),
            title: String::new(),
            slug: String::new(),
            latex: "x".into(),
        };
        add(&mut archive, &puzzle(5, "differential", "HARD"));
        add(&mut archive, &puzzle(5, "integral", "BEGINNER"));
        add(&mut archive, &puzzle(6, "integral", "EASY"));
        add(&mut archive, &puzzle(4, "integral", "EASY"));
        add(&mut archive, &puzzle(5, "derivative", "MEDIUM"));
        let days: Vec<u64> = archive["days"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["day"].as_u64().unwrap())
            .collect();
        assert_eq!(days, [6, 5, 4]);
        let kinds: Vec<String> = archive["days"][1]["problems"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                format!(
                    "{} {}",
                    p["kind"].as_str().unwrap(),
                    p["level"].as_str().unwrap()
                )
            })
            .collect();
        assert_eq!(
            kinds,
            [
                "integral BEGINNER",
                "derivative MEDIUM",
                "differential HARD"
            ]
        );
    }

    #[test]
    fn fetching_on_open_starts_off() {
        assert_eq!(empty()["fetchOnOpen"], false);
        assert_eq!(tidy(&json!({}))["fetchOnOpen"], false);
        assert_eq!(tidy(&json!({"fetchOnOpen": true}))["fetchOnOpen"], true);
    }

    #[test]
    fn stays_on_the_site() {
        assert!(allowed_url("https://dailyintegral.com/play/easy"));
        assert!(!allowed_url("https://dailyintegral.com.evil.example/"));
        assert!(!allowed_url("https://example.com/"));
    }
}
