// Release builds are a windowed app with no console window behind them.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod gui;
mod headless;
mod ui;

const USAGE: &str = "\
graphcalc (Phase 0 prototype)

USAGE:
    graphcalc                          open the calculator window
    graphcalc --render-png FILE [WxH]  draw the graph offscreen to a PNG and exit
    graphcalc --smoke-test SECS FILE   open the window, check the editor panel and
                                       graph both start, write the result to FILE
    graphcalc --opaque-window          open the window without transparency
                                       (try this if the editor panel looks wrong)
    graphcalc --dump-panel-html FILE   save the expression panel page, for testing
                                       it in a browser";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(&args) {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("error: {message}");
            1
        }
    };
    std::process::exit(code);
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => {
            println!("{USAGE}");
            Ok(())
        }
        Some("--render-png") => {
            let path = args.get(1).ok_or("--render-png needs a file name")?;
            let (w, h) = match args.get(2) {
                Some(size) => parse_size(size)?,
                None => (1200, 800),
            };
            headless::render_png(path, w, h)
        }
        Some("--dump-panel-html") => {
            let path = args.get(1).ok_or("--dump-panel-html needs a file name")?;
            std::fs::write(path, ui::panel_html()).map_err(|e| format!("{path}: {e}"))
        }
        _ => run_gui(args),
    }
}

#[cfg(windows)]
fn run_gui(args: &[String]) -> Result<(), String> {
    let mut options = gui::Options::default();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--opaque-window" => options.opaque_window = true,
            "--smoke-test" => {
                let secs = rest.next().ok_or("--smoke-test needs seconds")?;
                let secs: f64 = secs.parse().map_err(|_| format!("bad seconds: {secs}"))?;
                let out = rest.next().ok_or("--smoke-test needs an output file")?;
                options.smoke_test = Some((std::time::Duration::from_secs_f64(secs), out.into()));
            }
            other => return Err(format!("unknown argument: {other}\n\n{USAGE}")),
        }
    }
    gui::run(options)
}

#[cfg(not(windows))]
fn run_gui(_args: &[String]) -> Result<(), String> {
    Err(format!(
        "the calculator window is Windows-only for now\n\n{USAGE}"
    ))
}

fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once('x')
        .ok_or_else(|| format!("size must look like 1200x800, got {s}"))?;
    let parse = |v: &str| v.parse::<u32>().ok().filter(|v| (16..=8192).contains(v));
    match (parse(w), parse(h)) {
        (Some(w), Some(h)) => Ok((w, h)),
        _ => Err(format!("size must look like 1200x800, got {s}")),
    }
}
