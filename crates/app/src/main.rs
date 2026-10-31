// Release builds are a windowed app with no console window behind them.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod docs;
mod engine;
mod files;
#[cfg(windows)]
mod gui;
mod headless;
mod interact;
mod solve;
mod ui;

const USAGE: &str = "\
graphcalc (Phase 1 preview)

USAGE:
    graphcalc [FILE]                   open the calculator window, with a
                                       saved graph if one is given
    graphcalc --render-png FILE [WxH] [LATEX...] [--select=N] [--hover=X,Y] [--3d]
                                       graph the LaTeX rows (default y=sin x)
                                       offscreen to a PNG and exit
    graphcalc --bench [WxH] [FRAMES]   time a simulated slider drag (analyze,
                                       build and GPU render per frame) against
                                       the 60 fps budget
    graphcalc --bench-implicit [WxH] [FRAMES]
                                       time the implicit benchmark set: conics,
                                       dense curves and shaded inequalities
    graphcalc --bench-ticker FILE [TICKS]
                                       run a graph's ticker (default 100 times)
                                       and time each run against its step
    graphcalc --bench-suite [FRAMES]   time the Desmos comparison graphs in bench/
                                       (open, slider drag, pan, zoom) and print a
                                       table to set beside bench/desmos-bench.js
                                       (GRAPHCALC_SIZE=260x540 sets the graph size)
    graphcalc --bench-suite-3d [FRAMES]
                                       the same for the 3D graphs in bench/3d/
                                       (open, slider, turn, zoom), to set beside
                                       bench/desmos-3d-bench.js
    graphcalc --smoke-test SECS FILE   open the window, check the editor panel and
                                       graph both start, write the result to FILE
    graphcalc --opaque-window          open the window without transparency
                                       (try this if the editor panel looks wrong)
    graphcalc --dump-panel-html FILE   save the expression panel page, for testing
                                       it in a browser
    graphcalc --export-desmos FILE     print a graph as Desmos's Calc.setState
                                       takes it (warnings go to stderr)
    graphcalc --analyze                read the panel's rows as JSON on stdin and
                                       print the engine's reply, for testing
    graphcalc --solve LATEX [OP [SEEDS [DERIVS...]]]
                                       run the Solve button on one row and print
                                       the working as JSON (OP as in its choices,
                                       such as factor or solve:x)";

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
            let mut rest = &args[2..];
            let (w, h) = match rest.first().map(|s| parse_size(s)) {
                Some(Ok(size)) => {
                    rest = &rest[1..];
                    size
                }
                _ => (1200, 800),
            };
            headless::render_png(path, w, h, rest)
        }
        Some("--analyze") => {
            let mut input = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut input)
                .map_err(|e| e.to_string())?;
            let rows: serde_json::Value =
                serde_json::from_str(&input).map_err(|e| format!("bad JSON: {e}"))?;
            println!("{}", engine::Engine::default().update(&rows));
            Ok(())
        }
        Some("--solve") => {
            let latex = args.get(1).ok_or("--solve needs a row's LaTeX")?;
            // A differential equation's initial point and derivatives may follow.
            let row = serde_json::json!([{
                "id": 0,
                "latex": latex,
                "seeds": args.get(3),
                "derivs": args.get(4..).unwrap_or_default(),
            }]);
            let reply = solve::reply(&row, 0, latex, args.get(2).map(String::as_str));
            println!("{reply:#}");
            Ok(())
        }
        Some("--bench" | "--bench-implicit") => {
            let implicit = args[0] == "--bench-implicit";
            let (w, h) = match args.get(1) {
                Some(size) => parse_size(size)?,
                None => (1600, 1000),
            };
            let frames = match args.get(2) {
                Some(n) => n.parse().map_err(|_| format!("bad frame count: {n}"))?,
                None => 600,
            };
            if implicit {
                headless::bench_implicit(w, h, frames)
            } else {
                headless::bench_sliders(w, h, frames)
            }
        }
        Some("--bench-ticker") => {
            let path = args.get(1).ok_or("--bench-ticker needs a file name")?;
            let ticks = match args.get(2) {
                Some(n) => n.parse().map_err(|_| format!("bad tick count: {n}"))?,
                None => 100,
            };
            headless::bench_ticker(path, ticks)
        }
        Some("--bench-suite") => {
            let frames = match args.get(1) {
                Some(n) => n.parse().map_err(|_| format!("bad frame count: {n}"))?,
                None => 120,
            };
            headless::bench_suite(frames)
        }
        Some("--bench-suite-3d") => {
            let frames = match args.get(1) {
                Some(n) => n.parse().map_err(|_| format!("bad frame count: {n}"))?,
                None => 60,
            };
            headless::bench_suite_3d(frames)
        }
        Some("--desmos-script-3d") => {
            let path = args.get(1).ok_or("--desmos-script-3d needs a file name")?;
            std::fs::write(path, headless::desmos_script_3d()).map_err(|e| format!("{path}: {e}"))
        }
        Some("--desmos-script") => {
            let path = args.get(1).ok_or("--desmos-script needs a file name")?;
            std::fs::write(path, headless::desmos_script()).map_err(|e| format!("{path}: {e}"))
        }
        Some("--export-desmos") => {
            let path = args.get(1).ok_or("--export-desmos needs a file name")?;
            let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
            let (rows, view, _) = files::open_any(&text)?;
            let mut vp = graph_core::Viewport::new(1200.0, 800.0);
            if let Some(view) = view {
                view.apply(&mut vp);
            }
            let (state, warnings) = files::to_desmos(&rows, &vp, &files::space_of(&text));
            for w in warnings {
                eprintln!("warning: {w}");
            }
            println!("{state}");
            Ok(())
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
            // A graph file, as when one is double-clicked.
            other if !other.starts_with("--") => options.open = Some(other.into()),
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
