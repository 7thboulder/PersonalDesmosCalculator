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
  Higher orders work up to 6, such as `y'' = -0.3y' - y` or
  `\frac{d^3y}{dx^3} = -y`, with a box for each of y'(x₀) to y⁽ⁿ⁻¹⁾(x₀)
  (a number, or a list with one per point). Equations not solved for the
  highest derivative, such as `(y')^2 + y^2 = 1` or `x y' + y = 0`, are
  solved for it numerically, drawing every branch. Systems
  `x' = f, y' = g` (or `(x', y') = (f, g)`, or with `dx/dt`) in x, y and t
  draw a direction field and the trajectory through each point from t = 0.
- Piecewise definitions `{x < 0: -x, x}` and restrictions `y = x^2 {x > 0}`.
- Calculus: `d/dx sin(x^2)`, `f'(x)` and `f''(x)`, sums and products
  (type `sum` or `prod`, as in Desmos), and integrals such as
  `∫ from 0 to x of sin(t^2) dt`, including infinite bounds. Type `infinity`
  or `infty` for ∞ (or use ∞ under **functions** on the keypad), as in
  `∫ from 0 to ∞ of e^-x dx` or a restriction `{x < ∞}`. Derivatives are
  exact (symbolic); integrals use adaptive Gauss-Kronrod quadrature.
  An integral without bounds, such as `y = ∫ x e^x dx` or
  `f(x) = ∫ sec^3 x dx`, graphs its antiderivative (with the constant
  taken as 0), found exactly the same way the Solve button finds it.
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
- Matrices (+ then **matrix**, or type `matrix` in an empty row): a name
  and a grid of entries, which can use sliders. Enter moves to the next
  row of the grid, and the buttons under it add or remove rows and columns.
  Other rows use a matrix by name: `\det(A)`, `A^{-1}` (or `inv(A)`),
  `A^T`, `AB`, `2A`, `A^3`, `rref(A)`, `rank`, `trace`, `A[2, 1]`,
  `identity(3)`, and `[[1, 2], [3, 4]]` written out. Typing `matrix`
  anywhere else in an expression puts a grid there, such as
  `B = 2[grid] + A`: Enter goes to its next row (and out from the last),
  Shift+Enter adds a row, Shift+Space a column, and Backspace in an empty
  row or column takes it away. Matrix results show
  under the row, with simple fractions as fractions. `eigenvalues(A)` lists
  them (complex ones too) and `eigenvectors(A)` gives them as columns and
  draws each as an arrow on its dashed line. `A(1, 2)` moves a point and
  `A(\cos t, \sin t)` a curve. A 2×2 matrix has **Show transformation**:
  the grid, the unit square and the arrows for (1, 0) and (0, 1) move from
  where they are to where the matrix sends them, with a play button and a
  t slider, and the eigenvector lines dashed.
- Points of interest: select a row and its curve shows grey dots where it
  crosses the axes, turns, and meets other curves; hover or click one for
  its coordinates. Press and drag on any curve (explicit, parametric, polar
  or implicit) to trace it, and drag a point such as `(a, b)` to move its
  sliders.
- Solve with working (the **Solve** button at a row's top right, or
  Ctrl+Alt+S): exact algebra that runs only when asked, never while you
  type. An equation or inequality is solved for x (or any other letter),
  with each step written out: undoing operations, clearing fractions,
  factoring, the quadratic formula, rational roots, general solutions of
  trig equations with k, sign charts for inequalities, and a check that
  drops answers squaring added. An expression can be simplified (exact
  fractions and roots such as 5√2), expanded, factored, differentiated
  (each rule shown) or integrated (substitution, partial fractions, by
  parts, trig identities and reduction formulas for powers of sin, cos,
  tan and sec, trigonometric substitution for √(a² - x²), √(x² + a²) and
  √(x² - a²), completing the square, the t = tan(x/2) substitution,
  definite integrals and improper ones); `y = …` and `f(x) = …` rows also
  offer their roots. Every antiderivative is checked by differentiating
  it before it is shown. **Add to graph** puts the answer in a new row,
  and functions from other rows can be used.
- Solve also solves differential equations exactly, with steps:
  first order by direct integration, integrating factors, separating the
  variables, Bernoulli, exact (with an integrating factor if needed) and
  homogeneous substitutions; higher order by repeated integration,
  characteristic equations (real, repeated and complex roots, letters
  such as ω allowed) with undetermined coefficients or variation of
  parameters, Cauchy–Euler equations and reduction of order; and linear
  systems `x' = ax + by, y' = cx + dy` by eigenvalues. With the row's
  initial point (and initial derivatives) it finds the constants, so
  **Add to graph** draws the exact solution over the numerical one; the
  **General solution** chip keeps C, C₁, C₂, which become sliders. Each
  solution is checked in the equation before it is shown.
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
- Fractals and iteration: `mandelbrot()` draws the Mandelbrot set and
  `julia(-0.8+0.156i)` a Julia set (sliders work: `julia(a+bi)`), in the
  row's colour, on the GPU. They stay sharp zoomed in as far as the view
  goes (about 10¹³ times): the GPU's floats run out of digits much sooner,
  so the app iterates one point exactly and the GPU only each pixel's
  offset from it (perturbation). The iteration count rises with the zoom;
  `mandelbrot(5000)` or `julia(c, 5000)` sets it. With `f(x) = 3.7x(1-x)`
  defined, `cobweb(f, 0.2, 30)` draws the cobweb of 30 steps from 0.2 (add
  `y = f(x)` and `y = x` to see it against the curve); with
  `f(r, x) = rx(1-x)`, `bifurcation(f)` draws where x settles for each r
  along the x axis (`bifurcation(f, x_0)` starts the orbits at x_0).
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
  names one, and `l_1, l_2` runs several at once. An action can set a
  matrix too, as in `V \to V + aA`, rewriting its grid with the new
  entries. The + menu's **ticker**
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

## Math documents

The **Doc** button at the top of the expression list (or Ctrl+Alt+D) switches
the window to a document for typing math, such as homework or working through
a hard integral. **Graph** at the top of the document, or Ctrl+Alt+G, goes back.
The plan for this mode is in the
[document plan](https://claude.ai/artifact/4fZeSHt4D1JboMgPmCWN1A); this is
its first phase.

- Every line is math, typed exactly as in a Desmos row.
- `"` on an empty line makes a text line. In text, `$` starts math and `$`
  (or →) leaves it. `#` makes a heading (`# ` and `## ` at the start of text
  work too).
- Enter makes a new math line, Shift+Enter breaks a text line, and the arrow
  keys and Backspace move between and remove lines as in the expression list.
- Documents save as you type, as `.gcdoc` files in
  `Documents\Graph Calculator`. The sidebar lists that folder and one level of
  subfolders (one per class, say). Ctrl+N starts a new document, Ctrl+O opens
  one from anywhere and Ctrl+Shift+S saves a copy. Renaming the title renames
  the file.
- The app reopens whichever view and document were open last.
- Steps: Enter after an equation starts the next step, lined up on the `=`
  (or `<`, `≤`, `⇒`, ...) above. Enter on an empty step ends the steps, and
  Shift+Enter starts a line outside them. Ctrl+Enter carries the line down
  as the next step, starting from its `=`.
- Each group of steps that holds an equation gets a number such as (1).
  Typing `@` (in math or text) and a number inserts a reference, which
  follows the equation when lines move. Ctrl+Alt+P adds the next
  "Problem N" heading.
- Typing shortcuts: `;pd` gives ∂□/∂□, `;dd` d□/d□, `;dx` d/dx, `;lim` a
  limit, `vv` a vector arrow, `;cb` a cube root, `;nck` a binomial,
  `;pm` ±, `;oo` ∞, `;tf` ∴, `;=>` ⇒ and `;<=>` ⇔. □ marks a blank: the
  cursor goes to the first one and Tab moves to the next. **Shortcuts** in
  the document bar adds, changes, turns off or resets them (each one can
  also work in the graphing calculator), and right-clicking math makes a
  shortcut from it. They are saved in `%APPDATA%\graphcalc\shortcuts.json`.

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
| `crates/app/src/docs.rs` | Math document files (`.gcdoc`): checking, naming and listing them. The editor is `crates/app/ui/doc.js`. |
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
