//! The calculator window: wgpu draws the graph into the window itself, and a
//! WebView2 child covers the left side with the MathQuill expression panel.
//! This is the layout wry's own wgpu example uses on Windows.

use crate::engine::Engine;
use graph_core::{build_scene, TextAtlas, Viewport};
use graph_render::{new_instance, Area, Gpu, GraphRenderer, ShadingRenderer};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{Window, WindowId};
use wry::{Rect, WebView, WebViewBuilder};

/// Width of the expression panel in logical pixels, close to Desmos's default.
const PANEL_WIDTH: f64 = 380.0;
/// Zoom factor for one scroll-wheel notch.
const WHEEL_ZOOM: f64 = 1.2;
/// How often frame timings are pushed to the panel.
const STATS_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Default)]
pub struct Options {
    pub opaque_window: bool,
    /// Run for this long, then write a pass/fail report to the file and exit.
    pub smoke_test: Option<(Duration, PathBuf)>,
}

#[derive(Debug)]
pub enum UserEvent {
    /// A JSON message posted by the panel with `window.ipc.postMessage`.
    Ipc(String),
}

pub fn run(options: Options) -> Result<(), String> {
    let event_loop = EventLoop::<UserEvent>::with_user_event()
        .build()
        .map_err(|e| format!("could not start the event loop: {e}"))?;
    let mut app = App {
        proxy: event_loop.create_proxy(),
        smoke_deadline: options
            .smoke_test
            .as_ref()
            .map(|(d, _)| Instant::now() + *d),
        options,
        state: None,
        result: Ok(()),
    };
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    app.result
}

struct App {
    proxy: EventLoopProxy<UserEvent>,
    options: Options,
    smoke_deadline: Option<Instant>,
    state: Option<Running>,
    result: Result<(), String>,
}

struct Running {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gpu: Gpu,
    renderer: GraphRenderer,
    shader: ShadingRenderer,
    webview: Option<WebView>,
    viewport: Viewport,
    cursor: Option<PhysicalPosition<f64>>,
    dragging: bool,
    panel_ready: bool,
    engine: Engine,
    text: TextAtlas,
    /// Whether rows have arrived from the panel at least once.
    rows_received: bool,
    frames: u64,
    last_stats: Instant,
    /// Presentation times while the smoke test plays a slider.
    play_frames: Vec<Instant>,
    smoke_test: bool,
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        match Running::start(event_loop, &self.options, self.proxy.clone()) {
            Ok(state) => self.state = Some(state),
            Err(e) => {
                show_error(&format!("The calculator could not start.\n\n{e}"));
                self.result = Err(e);
                event_loop.exit();
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        if let (Some(state), UserEvent::Ipc(message)) = (&mut self.state, event) {
            state.on_panel_message(&message);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else { return };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => state.resize(size),
            WindowEvent::ScaleFactorChanged { .. } => state.resize(state.window.inner_size()),
            WindowEvent::RedrawRequested => state.render(),
            WindowEvent::CursorMoved { position, .. } => state.cursor_moved(position),
            WindowEvent::CursorLeft { .. } => state.cursor = None,
            WindowEvent::MouseInput {
                state: pressed,
                button: MouseButton::Left,
                ..
            } => {
                state.dragging =
                    pressed == ElementState::Pressed && state.cursor_in_graph().is_some();
                if state.dragging {
                    // Take keyboard focus back from the panel, as clicking the graph does in Desmos.
                    if let Some(webview) = &state.webview {
                        let _ = webview.focus_parent();
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let factor = match delta {
                    MouseScrollDelta::LineDelta(_, y) => WHEEL_ZOOM.powf(y as f64),
                    MouseScrollDelta::PixelDelta(p) => (p.y / 300.0).exp(),
                };
                state.zoom(factor);
            }
            WindowEvent::PinchGesture { delta, .. } => state.zoom(1.0 + delta),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(deadline) = self.smoke_deadline else {
            return;
        };
        if Instant::now() < deadline {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            return;
        }
        let (_, path) = self
            .options
            .smoke_test
            .as_ref()
            .expect("deadline implies smoke test");
        self.result = finish_smoke_test(self.state.as_ref(), path);
        event_loop.exit();
    }
}

impl Running {
    fn start(
        event_loop: &ActiveEventLoop,
        options: &Options,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self, String> {
        let mut attributes = Window::default_attributes()
            .with_title("Graphing Calculator (prototype)")
            .with_inner_size(LogicalSize::new(1280.0, 800.0))
            .with_min_inner_size(LogicalSize::new(640.0, 400.0));
        if !options.opaque_window {
            // Same setup as wry's wgpu example, so the panel composites over
            // the swap chain correctly.
            attributes = attributes.with_transparent(true).with_clip_children(false);
        }
        let window = Arc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| e.to_string())?,
        );

        let instance = new_instance();
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let gpu = Gpu::new(&instance, Some(&surface))?;
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .ok_or("the graphics adapter cannot draw to this window")?;
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&gpu.device, &config);
        let renderer = GraphRenderer::new(&gpu, config.format);
        let shader = ShadingRenderer::new(&gpu);

        let webview = WebViewBuilder::new()
            .with_bounds(panel_bounds(&window))
            .with_html(crate::ui::panel_html())
            .with_devtools(cfg!(debug_assertions))
            .with_ipc_handler(move |request| {
                let _ = proxy.send_event(UserEvent::Ipc(request.body().clone()));
            })
            .build_as_child(&window);
        let webview = match webview {
            Ok(webview) => Some(webview),
            Err(e) => {
                show_error(&format!(
                    "The expression panel needs Microsoft Edge WebView2, which could not start ({e}).\n\n\
                     It comes with Windows 11 and up-to-date Windows 10. You can install it from\n\
                     https://developer.microsoft.com/microsoft-edge/webview2/\n\n\
                     The graph will still open without the panel."
                ));
                None
            }
        };

        let mut state = Self {
            window,
            surface,
            config,
            gpu,
            renderer,
            shader,
            webview,
            viewport: Viewport::new(1.0, 1.0),
            cursor: None,
            dragging: false,
            panel_ready: false,
            engine: Engine::default(),
            text: TextAtlas::system(),
            rows_received: false,
            frames: 0,
            last_stats: Instant::now() - STATS_INTERVAL,
            play_frames: Vec::new(),
            smoke_test: options.smoke_test.is_some(),
        };
        let area = state.graph_area();
        state.viewport = Viewport::new(area.width as f64, area.height as f64);
        state.window.request_redraw();
        Ok(state)
    }

    /// The graph fills everything right of the panel.
    fn graph_area(&self) -> Area {
        let width = self.config.width;
        let panel = if self.webview.is_some() {
            ((PANEL_WIDTH * self.window.scale_factor()).round() as u32).min(width.saturating_sub(1))
        } else {
            0
        };
        Area {
            x: panel,
            y: 0,
            width: width - panel,
            height: self.config.height,
        }
    }

    fn cursor_in_graph(&self) -> Option<(f64, f64)> {
        let c = self.cursor?;
        let area = self.graph_area();
        let x = c.x - area.x as f64;
        (x >= 0.0).then_some((x, c.y))
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return; // Minimised.
        }
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.gpu.device, &self.config);
        if let Some(webview) = &self.webview {
            let _ = webview.set_bounds(panel_bounds(&self.window));
        }
        let area = self.graph_area();
        self.viewport.resize(area.width as f64, area.height as f64);
        self.window.request_redraw();
    }

    fn cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        if let (true, Some(previous)) = (self.dragging, self.cursor) {
            self.viewport
                .pan_px(position.x - previous.x, position.y - previous.y);
            self.window.request_redraw();
        }
        self.cursor = Some(position);
    }

    fn zoom(&mut self, factor: f64) {
        let area = self.graph_area();
        // Zoom around the cursor, or the centre when it is over the panel.
        let (x, y) = self
            .cursor_in_graph()
            .unwrap_or((area.width as f64 / 2.0, area.height as f64 / 2.0));
        self.viewport.zoom_at(x, y, factor);
        self.window.request_redraw();
    }

    fn render(&mut self) {
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.gpu.device, &self.config);
                self.window.request_redraw();
                return;
            }
            _ => return,
        };
        let start = Instant::now();
        let scale = self.window.scale_factor() as f32;
        let (viewport, text) = (&self.viewport, &mut self.text);
        let scene = self
            .engine
            .with_plot(|curves, implicits| build_scene(viewport, scale, curves, implicits, text));
        let scene_time = start.elapsed();

        let view = frame.texture.create_view(&Default::default());
        let size = (self.config.width, self.config.height);
        let area = self.graph_area();
        // The shader works in target pixels, so the graph area's offset is
        // folded into the world position of the target's own first pixel.
        let origin = self.viewport.to_world(-(area.x as f64), 0.0);
        let shadings = self
            .engine
            .shadings(origin, self.viewport.units_per_px, scale);
        self.renderer.draw_scene(
            &self.gpu,
            &view,
            size,
            area,
            &scene,
            &self.text,
            &shadings,
            &mut self.shader,
        );
        self.window.pre_present_notify();
        self.gpu.queue.present(frame);
        self.frames += 1;
        if self.smoke_test && self.rows_received {
            self.play_frames.push(Instant::now());
        }

        if self.panel_ready && self.last_stats.elapsed() >= STATS_INTERVAL {
            self.last_stats = Instant::now();
            self.run_script(&format!(
                "window.engine.stats({:.3}, {:.3}, {})",
                scene_time.as_secs_f64() * 1000.0,
                start.elapsed().as_secs_f64() * 1000.0,
                scene.vertices.len()
            ));
        }
    }

    fn on_panel_message(&mut self, message: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(message) else {
            return;
        };
        match value["type"].as_str() {
            Some("ready") => {
                self.panel_ready = true;
                let info = format!(
                    "{}, {}x MSAA",
                    self.gpu.describe(),
                    self.renderer.sample_count()
                );
                let info = serde_json::to_string(&info).unwrap_or_default();
                self.run_script(&format!("window.engine.info({info})"));
                if self.smoke_test {
                    // Add a slider and play it, so the report shows the frame
                    // rate while the graph animates.
                    self.run_script("window.engine.demo()");
                }
                self.window.request_redraw();
            }
            Some("rows") => {
                let results = self.engine.update(&value["rows"]);
                self.rows_received = true;
                self.run_script(&format!("window.engine.analysis({results})"));
                self.window.request_redraw();
            }
            _ => {}
        }
    }

    fn run_script(&self, js: &str) {
        if let Some(webview) = &self.webview {
            let _ = webview.evaluate_script(js);
        }
    }
}

fn panel_bounds(window: &Window) -> Rect {
    let height = window
        .inner_size()
        .to_logical::<f64>(window.scale_factor())
        .height;
    Rect {
        position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
        size: wry::dpi::LogicalSize::new(PANEL_WIDTH, height).into(),
    }
}

fn finish_smoke_test(state: Option<&Running>, path: &std::path::Path) -> Result<(), String> {
    let (ok, report) = match state {
        None => (false, "window did not start".to_owned()),
        Some(s) => {
            // Frame rate over the second half of the run, while the demo
            // slider plays.
            let tail = &s.play_frames[s.play_frames.len() / 2..];
            let fps = match (tail.first(), tail.last()) {
                (Some(a), Some(b)) if b > a => (tail.len() - 1) as f64 / (*b - *a).as_secs_f64(),
                _ => 0.0,
            };
            (
                s.panel_ready && s.frames > 0 && !s.engine.curves.is_empty(),
                format!(
                    "adapter: {}\nframes rendered: {}\npanel ready: {}\nrows from panel: {:?}\n\
                     curves drawn: {}\nframes per second while a slider plays: {fps:.1}\n",
                    s.gpu.describe(),
                    s.frames,
                    s.panel_ready,
                    s.engine.latex,
                    s.engine.curves.len(),
                ),
            )
        }
    };
    let report = format!(
        "{}\n{report}",
        if ok {
            "SMOKE TEST PASSED"
        } else {
            "SMOKE TEST FAILED"
        }
    );
    std::fs::write(path, &report).map_err(|e| format!("{}: {e}", path.display()))?;
    if ok {
        Ok(())
    } else {
        Err(report)
    }
}

fn show_error(message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let wide = |s: &str| {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (text, title) = (wide(message), wide("Graphing Calculator"));
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
