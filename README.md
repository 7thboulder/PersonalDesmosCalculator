# Personal Desmos Calculator

A native Windows graphing calculator that looks and types like Desmos, but does
its math and drawing with the machine's own CPU and GPU instead of a browser.
Private, personal-use project. The full plan lives in the
[plan doc](https://claude.ai/artifact/DZCzUyaNRYjCzz5fdBDS7n).

**Status: Phase 0 prototype.** The window opens with a Desmos-style expression
list on the left (the same MathQuill editor Desmos uses) and a GPU-drawn graph
on the right. The graph always shows y = sin(x) for now. What you type is sent
to the Rust engine, and graphing it arrives in Phase 1.

## Trying it on Windows 10 or 11

1. Open the latest successful run of the **Windows build** workflow under the
   repository's Actions tab.
2. Download the `graphcalc-windows` artifact and unzip it.
3. Double-click `graphcalc.exe`. Nothing needs installing.

Windows may show "Windows protected your PC" because the file is not signed.
Click **More info**, then **Run anyway**.

Things to check:

- The expression list appears on the left and you can type math into it
  (`/` makes a fraction, `^` an exponent, `sqrt` a root, Enter a new row).
- Dragging the graph pans it and the scroll wheel zooms around the cursor.
- The panel's footer names your graphics card and shows the frame time.

If the panel is missing or drawn wrongly, try `graphcalc.exe --opaque-window`
from a command prompt and note whether that looks better.

## Layout

| Path | What it is |
| --- | --- |
| `crates/graph-core` | Viewport, grid, curve sampling and line tessellation. Pure Rust, unit tested. |
| `crates/graph-render` | Draws scenes with wgpu (DirectX 12 on Windows), on screen or offscreen. |
| `crates/app` | The `graphcalc` executable: window, input, and the WebView2 expression panel. |
| `crates/app/ui` | Panel HTML, CSS and JS, plus vendored MathQuill 0.10.1 (MPL-2.0) and jQuery 3.7.1 (MIT). Everything is embedded in the exe. |

## Developing

```sh
cargo test --workspace                          # runs everywhere, GPU test skips if no adapter
cargo build --release -p app                    # target/release/graphcalc(.exe)
graphcalc --render-png out.png 1200x800         # offscreen render, works without a window
graphcalc --dump-panel-html panel.html          # the panel page, for testing in a browser
cargo check -p app --target x86_64-pc-windows-msvc   # type-check the window code from Linux or macOS
```

The window code is Windows-only. On other systems the binary supports the
offscreen commands above.
