# Personal Desmos Calculator

A native Windows graphing calculator that looks and types like Desmos, but does
its math and drawing with the machine's own CPU and GPU instead of a browser.
Private, personal-use project. The full plan lives in the
[plan doc](https://claude.ai/artifact/DZCzUyaNRYjCzz5fdBDS7n).

**Status: Phases 1 to 4 merged; 3D graphing under way.** The window opens with a Desmos-style expression
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
  `L[2]`, filters `L[L > 0]`, comprehensions `[n^2 for n = [1...5]]`, and `total`, `mean`,
  `length`, `min`, `max` and friends. A list in a curve draws one curve per
  element, as `y = [1, 2, 3]x` does in Desmos.
- Differential equations `y' = f(x, y)` (or `dy/dx = f(x, y)`): a slope
  field over the view, and a solution curve through each point in the
  **through** box under the row, such as `(0, 1)` or `[(0, 1), (2, -3)]`.
  Drag a point on the graph to move its solution; points written with
  sliders, such as `(a, b)`, move those sliders. Solutions are integrated
  with adaptive Runge-Kutta (Dormand-Prince) to a tenth of a pixel.
- Piecewise definitions `{x < 0: -x, x}` and restrictions `y = x^2 {x > 0}`.
- Calculus: `d/dx sin(x^2)`, `f'(x)` and `f''(x)`, sums and products
  (type `sum` or `prod`, as in Desmos), and integrals such as
  `∫ from 0 to x of sin(t^2) dt`, including infinite bounds. Derivatives are
  exact (symbolic); integrals use adaptive Gauss-Kronrod quadrature.
- 3D graphs: the **2D / 3D** switch at the top of the list turns the graph
  into Desmos 3D's box. `z = f(x, y)` (or a bare expression in x and y) is
  drawn as a lit surface, trimmed to the box, with r and θ measured around
  the z axis. Drag to turn the box, Shift+drag to slide it, and scroll to
  zoom. Each graph is either 2D or 3D, as in Desmos, and its file remembers
  which. Points `(1, 2, 3)` (and lists of them, with `p.z`) are drawn as
  dots and `(cos t, sin t, t)` as a curve; flat points and parametric
  curves sit on the z = 0 plane. Any other equation, such as
  `x^2 + y^2 + z^2 = 25` or `y = x^2`, is drawn where both sides are
  equal, with ρ (distance from the origin) and φ (angle from +z) as in
  Desmos 3D, so `ρ = 5` is a sphere and `r = 3` a cylinder. A point that
  reads u and v, such as `(cos 2πu, sin 2πu, v)`, is a surface over
  0 ≤ u, v ≤ 1. An inequality such as `x^2 + y^2 + z^2 < 25` or
  `-1 < z < 1` is a see-through solid, closed off by the box's walls where
  it meets them; restrictions such as `\{0 ≤ x ≤ 1\}` cut it, and bounds
  that are numbers on x, y and z make exact boxes (one per list element, as
  in a 3D Riemann sum). `f(x, y) = …` is drawn as a surface, and `polygon`
  takes 3D points.
- Notes (+ then **note**, Ctrl+Alt+O, or typing `"` in an empty row): a row
  of plain text between expressions. Shift+Enter breaks the line; Enter
  starts a new expression below.
- Tables (+ then **table**): each column after the first is plotted against
  it, a column named `y_1` defines that list for other rows, and a column
  headed by a formula such as `x_1^2` fills itself in. Enter moves down,
  arrow keys move between cells, and moving right from the last column adds
  one. Each column has its own color circle and style, including **Points**
  and **Lines** toggles (point lists in expressions get them too).
- Points of interest: select a row and its curve shows grey dots where it
  crosses the axes, turns, and meets other curves; hover or click one for
  its coordinates. Press and drag on any curve (explicit, parametric, polar
  or implicit) to trace it, and drag a point such as `(a, b)` to move its
  sliders.
- Undo and redo: Ctrl+Z, Ctrl+Y (or Ctrl+Shift+Z) and the arrows at the top.
- On-screen keypad: the keyboard button at the bottom left (or Ctrl+Alt+K)
  opens Desmos's keypad, with its 123, ABC and functions panels. Its keys
  type into the field you were last in.
- Desmos's keyboard shortcuts: Ctrl+Alt+X adds an expression, Ctrl+Alt+T
  (or typing `table` in an empty row) a table, Ctrl+Shift+D deletes the
  current row, and Ctrl+Alt+E jumps to the list.
- Help: the **?** button at the top (or Ctrl+/) lists every keyboard
  shortcut and every function, with an example of each.
  A burst of typing or a slider drag undoes as one step.
- Saving and opening: the ☰ menu (or Ctrl+S, Ctrl+Shift+S, Ctrl+O) saves
  graphs as `.gcalc` files with their view, and `graphcalc FILE` opens one.
  The session is autosaved, so the next launch carries on where you left
  off, unsaved changes included; New and Open ask before dropping them.
- Complex numbers, always on: `i`, `e^{iθ}`, `3+2i` arithmetic and powers,
  `real`, `imag`, `conj`, `arg` and `abs`, and `sin`, `cos`, `tan` (and
  their reciprocals), `sinh`, `cosh`, `tanh`, `ln`, `log`, `exp` and `sqrt`,
  in lists too.
- Complex function plots: a row in z, such as `z^2 + 1`, `\sin z` or
  `f(z)` for a function f, colors the plane by its value at z = x + iy
  (domain coloring, drawn by the GPU). The hue is the angle of f(z), red
  where it is a positive real number, and the shade goes dark at zeros and
  light at poles, with faint steps where |f(z)| doubles. Hold the row's
  circle and pick **Grid** to see instead where the view's grid lines go
  under f, with the lines of constant real part brighter than those of
  constant imaginary part. A row that defines z makes it an ordinary
  variable again.
- List tools: `sort(L, keys)`, `join`, `unique`, `distance`, `midpoint`,
  comprehensions written `f(n) for n = L`, and `L[n].x`.
- Polygons: `polygon((0,0), (4,0), (2,3))`, or one per element with `for`,
  filled at 40% (the style menu's **Fill** slider changes it).
- Actions: `a \to a + 1` (type `->`) gets a **Run** button, `l = a \to 1`
  names one, and `l_1, l_2` runs several at once. The + menu's **ticker**
  runs an action over and over, every so many milliseconds.
- Regressions: `y_1 \sim m x_1 + b` (type `~`) fits every name that is not
  defined elsewhere, shows the values and R², draws the fitted curve, and
  stores the residuals as `e_1` (then `e_2`, ...). The fitted names can be
  used in other rows. Any model works, such as `y_1 \sim a e^{k x_1}`.
- Opening a Desmos graph: paste its desmos.com/calculator or desmos.com/3d
  link anywhere in the app (Ctrl+V), or use ☰ then **Import from Desmos**,
  and the app fetches it (a Desmos 3D graph opens as a 3D graph). Pasting the text `JSON.stringify(Calc.getState())` gives in Desmos's
  browser console works too, as does opening it saved as a `.json` or `.txt`
  file with Ctrl+O.
  Expressions, tables, notes, sliders, styles, the ticker and the view come
  across; folders and images are left out. Save writes a new `.gcalc` file.
- Variables (`a = 2`) and functions (`f(x) = x^2`, `g(a, b) = a - b`), defined
  in any order across rows.
- Undefined names offer **add slider** buttons. Slider rows get a slider with
  an editable range and step (click the -10 or 10) and a play button. While
  it plays, − and + change its speed and the loop button cycles through
  back and forth, loop forward, play once and play forever.
- Rows such as `2 + 3` or `b = 2a` show their value; mistakes show a warning
  triangle with Desmos-style wording on hover.
- Click a row's color circle to hide or show its curve; hold it (or
  right-click) for colors, solid, dashed or dotted lines, thickness and opacity.
- Axis numbers in Desmos's style.

Not yet: points of interest where two parametric or implicit curves meet
each other (they show where these meet y = f(x) curves and the axes).
Shading is computed by the GPU in single precision, so a
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
graphcalc --render-png out.png 900x700 --3d 'z=x^2-y^2'   # the same for a 3D graph
graphcalc --bench                               # slider-drag frame time against the 60 fps budget
graphcalc --bench-implicit                      # the same budget over eleven implicit and shaded relations
graphcalc --bench-suite-3d                      # time the 3D graphs in bench/3d (beside bench/desmos-3d-bench.js)
echo '[{"id":1,"latex":"1+1"}]' | graphcalc --analyze   # the engine's reply to panel rows
graphcalc --dump-panel-html panel.html          # the panel page, for testing in a browser
cargo check -p app --target x86_64-pc-windows-msvc   # type-check the window code from Linux or macOS
```

The window code is Windows-only. On other systems the binary supports the
offscreen commands above.
