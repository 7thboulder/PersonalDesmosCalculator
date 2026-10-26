# Personal Desmos Calculator

A native Windows graphing calculator that looks and types like Desmos, but does
its math and drawing with the machine's own CPU and GPU instead of a browser.
Private, personal-use project. The full plan lives in the
[plan doc](https://claude.ai/artifact/DZCzUyaNRYjCzz5fdBDS7n).

**Status: Phase 3 in progress.** The window opens with a Desmos-style expression
list on the left (the same MathQuill editor Desmos uses) and a GPU-drawn graph
on the right. What you type is graphed as you type it:

- Curves `y = f(x)`, `x = g(y)`, and bare expressions in x such as `x^2`.
- Implicit equations such as `x^2 + y^2 = 25` or `sin(xy) = cos(x)`, however
  densely they fold.
- Inequalities such as `y > x^2` or `x^2 - y^2 < 4`, with Desmos's shading on
  the side that satisfies them, and chains such as `-1 < y < x`.
- Points `(1, 2)`, point variables `p = (1, 2)` with `p.x` and `p.y`, and
  lists of points.
- Parametric curves `(cos t, sin t)` and polar curves `r = 2 sin(3θ)`, with an
  editable domain under the row (Desmos's defaults are 0 ≤ t ≤ 1 and
  0 ≤ θ ≤ 12π). `r` and `θ` also work inside relations, as in `r < 2`.
- Lists: `[1, 2, 3]`, ranges `[1...10]` and `[1, 3, ..., 11]`, indexing
  `L[2]`, comprehensions `[n^2 for n = [1...5]]`, and `total`, `mean`,
  `length`, `min`, `max` and friends. A list in a curve draws one curve per
  element, as `y = [1, 2, 3]x` does in Desmos.
- Piecewise definitions `{x < 0: -x, x}` and restrictions `y = x^2 {x > 0}`.
- Calculus: `d/dx sin(x^2)`, `f'(x)` and `f''(x)`, sums and products
  (type `sum` or `prod`, as in Desmos), and integrals such as
  `∫ from 0 to x of sin(t^2) dt`, including infinite bounds. Derivatives are
  exact (symbolic); integrals use adaptive Gauss-Kronrod quadrature.
- Tables (+ then **table**): each column after the first is plotted against
  it, a column named `y_1` defines that list for other rows, and a column
  headed by a formula such as `x_1^2` fills itself in. Enter moves down,
  arrow keys move between cells, and moving right from the last column adds
  one. Each column has its own color circle and style, including **Points**
  and **Lines** toggles (point lists in expressions get them too).
- Variables (`a = 2`) and functions (`f(x) = x^2`, `g(a, b) = a - b`), defined
  in any order across rows.
- Undefined names offer **add slider** buttons. Slider rows get a slider with
  an editable range and step (click the -10 or 10) and a play button.
- Rows such as `2 + 3` or `b = 2a` show their value; mistakes show a warning
  triangle with Desmos-style wording on hover.
- Click a row's color circle to hide or show its curve; hold it (or
  right-click) for colors, solid, dashed or dotted lines, thickness and opacity.
- Axis numbers in Desmos's style.

Not yet: points of interest,
trace and draggable points; saving and opening graphs. Those are the rest of
Phase 3. Shading is computed by the GPU in single precision, so a
very deep zoom on an implicit curve will eventually look grainy; a
double-precision fallback at that zoom is still to come.

## Trying it on Windows 10 or 11

1. Open the latest successful run of the **Windows build** workflow under the
   repository's Actions tab.
2. Download the `graphcalc-windows` artifact and unzip it.
3. Double-click `graphcalc.exe`. Nothing needs installing.

Windows may show "Windows protected your PC" because the file is not signed.
Click **More info**, then **Run anyway**.

Things to check:

- Type `y=mx+b`, press **all** under it, then drag and play the sliders.
- Type `x^2+y^2=a^2`, add the slider, and play it: the circle should stay
  smooth and keep up. Then try `y>x^2`, `\sin\left(xy\right)=\cos\left(x\right)`
  and a dashed or dotted implicit curve from the style menu.
- Typing behaves like Desmos (`/` makes a fraction, `^` an exponent, `sqrt` a
  root, Enter a new row) and the results match what Desmos shows.
- Type `(cos t, sin t)`, change its domain to `2π`, and try a point list's
  point styles from the color circle's menu.
- Dragging the graph pans it and the scroll wheel zooms around the cursor.
- The panel's footer names your graphics card and shows the frame time.

If the panel is missing or drawn wrongly, try `graphcalc.exe --opaque-window`
from a command prompt and note whether that looks better.

## Layout

| Path | What it is |
| --- | --- |
| `crates/graph-expr` | The expression engine: Desmos-rules LaTeX parser, cross-row analysis, compiled curve programs, interval arithmetic and WGSL generation. Golden tests in `tests/golden.rs`. |
| `crates/graph-core` | Viewport, grid, curve sampling, the implicit quadtree tracer, line styles, axis labels and tessellation. Pure Rust, unit tested. |
| `crates/graph-render` | Draws scenes with wgpu (DirectX 12 on Windows), on screen or offscreen, including per-pixel shading of relations. |
| `crates/app` | The `graphcalc` executable: window, input, and the WebView2 expression panel. |
| `crates/app/ui` | Panel HTML, CSS and JS, plus vendored MathQuill 0.10.1 (MPL-2.0) and jQuery 3.7.1 (MIT). Everything is embedded in the exe. |

## Developing

```sh
cargo test --workspace                          # runs everywhere, GPU test skips if no adapter
cargo build --release -p app                    # target/release/graphcalc(.exe)
graphcalc --render-png out.png 1200x800 'y=x^2' # offscreen render of LaTeX rows, no window needed
graphcalc --bench                               # slider-drag frame time against the 60 fps budget
graphcalc --bench-implicit                      # the same budget over eleven implicit and shaded relations
echo '[{"id":1,"latex":"1+1"}]' | graphcalc --analyze   # the engine's reply to panel rows
graphcalc --dump-panel-html panel.html          # the panel page, for testing in a browser
cargo check -p app --target x86_64-pc-windows-msvc   # type-check the window code from Linux or macOS
```

The window code is Windows-only. On other systems the binary supports the
offscreen commands above.
