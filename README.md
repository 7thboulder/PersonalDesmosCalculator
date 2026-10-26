# Personal Desmos Calculator

A native Windows graphing calculator that looks and types like Desmos, but does
its math and drawing with the machine's own CPU and GPU instead of a browser.
Private, personal-use project. The full plan lives in the
[plan doc](https://claude.ai/artifact/DZCzUyaNRYjCzz5fdBDS7n).

**Status: Phase 1 preview.** The window opens with a Desmos-style expression
list on the left (the same MathQuill editor Desmos uses) and a GPU-drawn graph
on the right. What you type is graphed as you type it:

- Curves `y = f(x)`, `x = g(y)`, and bare expressions in x such as `x^2`.
- Variables (`a = 2`) and functions (`f(x) = x^2`, `g(a, b) = a - b`), defined
  in any order across rows.
- Undefined names offer **add slider** buttons. Slider rows get a slider with
  an editable range and step (click the -10 or 10) and a play button.
- Rows such as `2 + 3` or `b = 2a` show their value; mistakes show a warning
  triangle with Desmos-style wording on hover.
- Click a row's color circle to hide or show its curve; hold it (or
  right-click) for colors, solid, dashed or dotted lines, thickness and opacity.
- Axis numbers in Desmos's style.

Not yet: implicit equations (`x^2 + y^2 = 1`), inequalities, polar, points,
lists, derivatives, sums and integrals. Those rows say they arrive later.

## Trying it on Windows 10 or 11

1. Open the latest successful run of the **Windows build** workflow under the
   repository's Actions tab.
2. Download the `graphcalc-windows` artifact and unzip it.
3. Double-click `graphcalc.exe`. Nothing needs installing.

Windows may show "Windows protected your PC" because the file is not signed.
Click **More info**, then **Run anyway**.

Things to check:

- Type `y=mx+b`, press **all** under it, then drag and play the sliders.
- Typing behaves like Desmos (`/` makes a fraction, `^` an exponent, `sqrt` a
  root, Enter a new row) and the results match what Desmos shows.
- Dragging the graph pans it and the scroll wheel zooms around the cursor.
- The panel's footer names your graphics card and shows the frame time.

If the panel is missing or drawn wrongly, try `graphcalc.exe --opaque-window`
from a command prompt and note whether that looks better.

## Layout

| Path | What it is |
| --- | --- |
| `crates/graph-expr` | The expression engine: Desmos-rules LaTeX parser, cross-row analysis, compiled curve programs. Golden tests in `tests/golden.rs`. |
| `crates/graph-core` | Viewport, grid, curve sampling, line styles, axis labels and tessellation. Pure Rust, unit tested. |
| `crates/graph-render` | Draws scenes with wgpu (DirectX 12 on Windows), on screen or offscreen. |
| `crates/app` | The `graphcalc` executable: window, input, and the WebView2 expression panel. |
| `crates/app/ui` | Panel HTML, CSS and JS, plus vendored MathQuill 0.10.1 (MPL-2.0) and jQuery 3.7.1 (MIT). Everything is embedded in the exe. |

## Developing

```sh
cargo test --workspace                          # runs everywhere, GPU test skips if no adapter
cargo build --release -p app                    # target/release/graphcalc(.exe)
graphcalc --render-png out.png 1200x800 'y=x^2' # offscreen render of LaTeX rows, no window needed
graphcalc --bench                               # slider-drag frame time against the 60 fps budget
echo '[{"id":1,"latex":"1+1"}]' | graphcalc --analyze   # the engine's reply to panel rows
graphcalc --dump-panel-html panel.html          # the panel page, for testing in a browser
cargo check -p app --target x86_64-pc-windows-msvc   # type-check the window code from Linux or macOS
```

The window code is Windows-only. On other systems the binary supports the
offscreen commands above.
