//! The calculator window: wgpu draws the graph into the window itself, and a
//! WebView2 child covers the left side with the MathQuill expression panel.
//! This is the layout wry's own wgpu example uses on Windows.

use crate::docs;
use crate::engine::Engine;
use crate::files::{self, BoxView, Document, Space};
use crate::interact::{Effect, Interaction};
use graph_core::{build_scene_cached, LineCache, TextAtlas, View3, Viewport};
use graph_expr::Mode;
use graph_render::{new_instance, Area, Gpu, GraphRenderer, Renderer3, ShadingRenderer};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{CursorIcon, Window, WindowId};
use wry::{Rect, WebView, WebViewBuilder};

/// Width of the expression panel in logical pixels, close to Desmos's default.
const PANEL_WIDTH: f64 = 380.0;
/// Zoom factor for one scroll-wheel notch.
const WHEEL_ZOOM: f64 = 1.2;
/// How often frame timings are pushed to the panel.
const STATS_INTERVAL: Duration = Duration::from_millis(250);
/// How long after a change the session is autosaved.
const AUTOSAVE_DELAY: Duration = Duration::from_secs(1);

#[derive(Default)]
pub struct Options {
    pub opaque_window: bool,
    /// Run for this long, then write a pass/fail report to the file and exit.
    pub smoke_test: Option<(Duration, PathBuf)>,
    /// A graph file to open instead of restoring the last session.
    pub open: Option<PathBuf>,
}

#[derive(Debug)]
pub enum UserEvent {
    /// A JSON message posted by the panel with `window.ipc.postMessage`.
    Ipc(String),
    /// desmos.com's reply for a pasted link, fetched off the UI thread.
    Fetched {
        url: String,
        reply: Result<String, String>,
    },
    /// WebView2 finished writing a PDF of the document.
    PdfSaved {
        path: PathBuf,
        result: Result<(), String>,
        open: bool,
    },
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
    renderer3: Renderer3,
    webview: Option<WebView>,
    viewport: Viewport,
    /// The 3D box, used when the graph is 3D.
    view3: View3,
    cursor: Option<PhysicalPosition<f64>>,
    dragging: bool,
    panel_ready: bool,
    engine: Engine,
    interact: Interaction,
    text: TextAtlas,
    /// Curve lines kept from earlier frames, so panning is cheap.
    lines: LineCache,
    /// Whether rows have arrived from the panel at least once.
    rows_received: bool,
    frames: u64,
    last_stats: Instant,
    /// Presentation times while the smoke test plays a slider.
    play_frames: Vec<Instant>,
    smoke_test: bool,
    modifiers: ModifiersState,
    doc: Document,
    /// The rows as the panel last sent them: the graph to save.
    rows: serde_json::Value,
    autosave_due: Option<Instant>,
    /// A file named on the command line, opened once the panel is ready.
    open_on_start: Option<PathBuf>,
    proxy: EventLoopProxy<UserEvent>,
    /// The Document view is showing: the panel covers the whole window.
    doc_view: bool,
    /// The open document and the library folder, remembered across starts.
    docs: docs::State,
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
        let Some(state) = &mut self.state else { return };
        match event {
            UserEvent::Ipc(message) => state.on_panel_message(&message),
            UserEvent::Fetched { url, reply } => state.fetched(&url, reply),
            UserEvent::PdfSaved { path, result, open } => state.pdf_saved(path, result, open),
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = &mut self.state else { return };
        match event {
            WindowEvent::CloseRequested => {
                state.autosave();
                event_loop.exit();
            }
            WindowEvent::ModifiersChanged(m) => state.modifiers = m.state(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Character(c),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } if state.modifiers.control_key() => {
                let shift = state.modifiers.shift_key();
                let alt = state.modifiers.alt_key();
                match c.to_lowercase().as_str() {
                    // The graph has the keyboard, so the panel never saw it.
                    "d" if alt => state.run_script("window.doc && window.doc.show(true)"),
                    _ if alt => {}
                    "s" if shift => state.file_command("saveAs"),
                    "s" => state.file_command("save"),
                    "o" => state.file_command("open"),
                    _ => {}
                }
            }
            WindowEvent::Resized(size) => state.resize(size),
            WindowEvent::ScaleFactorChanged { .. } => state.resize(state.window.inner_size()),
            WindowEvent::RedrawRequested => state.render(),
            WindowEvent::CursorMoved { position, .. } => state.cursor_moved(position),
            WindowEvent::CursorLeft { .. } => {
                state.cursor = None;
                let effect = state.interact.left();
                state.apply(effect);
            }
            WindowEvent::MouseInput {
                state: pressed,
                button: MouseButton::Left,
                ..
            } => state.mouse_button(pressed == ElementState::Pressed),
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
        if let Some(state) = &mut self.state {
            if let Some(due) = state.autosave_due {
                if Instant::now() >= due {
                    state.autosave();
                } else if self.smoke_deadline.is_none() {
                    event_loop.set_control_flow(ControlFlow::WaitUntil(due));
                    return;
                }
            }
        }
        let Some(deadline) = self.smoke_deadline else {
            event_loop.set_control_flow(ControlFlow::Wait);
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
        let renderer3 = renderer.renderer_3d(&gpu);

        let ipc_proxy = proxy.clone();
        let webview = WebViewBuilder::new()
            .with_bounds(panel_bounds(&window, false))
            .with_html(crate::ui::panel_html())
            .with_devtools(cfg!(debug_assertions))
            .with_ipc_handler(move |request| {
                let _ = ipc_proxy.send_event(UserEvent::Ipc(request.body().clone()));
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
            renderer3,
            webview,
            viewport: Viewport::new(1.0, 1.0),
            view3: View3::new(1.0, 1.0),
            cursor: None,
            dragging: false,
            panel_ready: false,
            engine: Engine::default(),
            interact: Interaction::default(),
            text: TextAtlas::system(),
            lines: LineCache::default(),
            rows_received: false,
            frames: 0,
            last_stats: Instant::now() - STATS_INTERVAL,
            play_frames: Vec::new(),
            smoke_test: options.smoke_test.is_some(),
            modifiers: ModifiersState::empty(),
            doc: Document::default(),
            rows: serde_json::Value::Array(Vec::new()),
            autosave_due: None,
            open_on_start: options.open.clone(),
            proxy,
            doc_view: false,
            docs: docs::read_state(),
        };
        let area = state.graph_area();
        state.viewport = Viewport::new(area.width as f64, area.height as f64);
        state.view3 = View3::new(area.width as f64, area.height as f64);
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
            let _ = webview.set_bounds(panel_bounds(&self.window, self.doc_view));
        }
        let area = self.graph_area();
        self.viewport.resize(area.width as f64, area.height as f64);
        self.view3.resize(area.width as f64, area.height as f64);
        self.window.request_redraw();
    }

    fn three_d(&self) -> bool {
        self.engine.mode == Mode::ThreeD
    }

    /// The graph's mode and 3D view, for saving.
    fn space(&self) -> Space {
        Space {
            three_d: self.three_d(),
            view: Some(BoxView::of(&self.view3)),
        }
    }

    /// Switches between a flat graph and a 3D one. The rows stay as they
    /// are and are read again for the new mode.
    fn set_mode(&mut self, three_d: bool) {
        self.engine.mode = if three_d { Mode::ThreeD } else { Mode::Flat };
        self.doc.mode_changed(three_d);
        self.update_title();
        self.run_script(&format!("window.engine.mode({three_d})"));
        if self.rows_received {
            let results = self.engine.update(&self.rows);
            self.run_script(&format!("window.engine.analysis({results})"));
        }
        self.window.request_redraw();
    }

    /// Takes the mode and 3D view of a graph being opened.
    fn apply_space(&mut self, space: Space) {
        let area = self.graph_area();
        self.view3 = View3::new(area.width as f64, area.height as f64);
        if let Some(view) = space.view {
            view.apply(&mut self.view3);
        }
        self.set_mode(space.three_d);
    }

    fn cursor_moved(&mut self, position: PhysicalPosition<f64>) {
        let previous = self.cursor;
        self.cursor = Some(position);
        let Some((x, y)) = self.cursor_in_graph() else {
            return;
        };
        if self.three_d() {
            // Dragging turns the box; with Shift held it slides.
            if let (true, Some(previous)) = (self.dragging, previous) {
                let (dx, dy) = (position.x - previous.x, position.y - previous.y);
                if self.modifiers.shift_key() {
                    self.view3.pan_px(dx, dy);
                } else {
                    self.view3.turn_px(dx, dy);
                }
                self.window.request_redraw();
            }
            return;
        }
        let effect = self.interact.moved(&self.engine, &self.viewport, [x, y]);
        let pointer = self
            .interact
            .over_something(&self.engine, &self.viewport, [x, y]);
        self.window.set_cursor(if pointer {
            CursorIcon::Pointer
        } else {
            CursorIcon::Default
        });
        if effect == Effect::Pan {
            if let (true, Some(previous)) = (self.dragging, previous) {
                self.viewport
                    .pan_px(position.x - previous.x, position.y - previous.y);
                self.window.request_redraw();
            }
            return;
        }
        self.apply(effect);
    }

    fn mouse_button(&mut self, pressed: bool) {
        if !pressed {
            self.dragging = false;
            self.interact.release();
            return;
        }
        let Some((x, y)) = self.cursor_in_graph() else {
            return;
        };
        // Take keyboard focus back from the panel, as clicking the graph
        // does in Desmos.
        if let Some(webview) = &self.webview {
            let _ = webview.focus_parent();
        }
        if self.three_d() {
            self.dragging = true;
            return;
        }
        self.interact.scale = self.window.scale_factor();
        let effect = self.interact.press(&self.engine, &self.viewport, [x, y]);
        self.dragging = true;
        self.apply(effect);
    }

    /// Carries out what a mouse event asked for.
    fn apply(&mut self, effect: Effect) {
        match effect {
            Effect::None | Effect::Pan => return,
            Effect::Redraw => {}
            Effect::Select(row) => {
                if self.engine.selected != Some(row) {
                    self.engine.selected = Some(row);
                    self.interact.selection_changed();
                }
                self.run_script(&format!("window.engine.select({row})"));
            }
            Effect::Deselect => {
                self.engine.selected = None;
                self.interact.selection_changed();
                self.run_script("window.engine.select(null)");
            }
            Effect::SetSliders(values) => {
                let pairs: Vec<String> = values
                    .iter()
                    .map(|(row, v)| format!("[{row},{v}]"))
                    .collect();
                self.run_script(&format!("window.engine.setSliders([{}])", pairs.join(",")));
            }
            Effect::SetSeeds(row, latex) => {
                let latex = serde_json::Value::String(latex).to_string();
                self.run_script(&format!("window.engine.setSeeds({row},{latex})"));
            }
        }
        self.window.request_redraw();
    }

    fn zoom(&mut self, factor: f64) {
        if self.three_d() {
            self.view3.zoom(factor);
            self.window.request_redraw();
            return;
        }
        let area = self.graph_area();
        // Zoom around the cursor, or the centre when it is over the panel.
        let (x, y) = self
            .cursor_in_graph()
            .unwrap_or((area.width as f64 / 2.0, area.height as f64 / 2.0));
        self.viewport.zoom_at(x, y, factor);
        self.window.request_redraw();
    }

    fn render(&mut self) {
        // The document covers the graph, so there is nothing to draw.
        if self.doc_view {
            return;
        }
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
        if self.three_d() {
            let view = frame.texture.create_view(&Default::default());
            let size = (self.config.width, self.config.height);
            let area = self.graph_area();
            let (gpu, renderer, renderer3) = (&self.gpu, &mut self.renderer, &mut self.renderer3);
            let mut scene_time = Duration::ZERO;
            let vertices = self.engine.with_frame_3d(
                &self.view3,
                scale,
                &mut self.text,
                |frame, overlay, text| {
                    scene_time = start.elapsed();
                    renderer.draw_3d(gpu, &view, size, area, frame, overlay, text, renderer3);
                    frame
                        .surfaces
                        .iter()
                        .map(|m| m.indices.len())
                        .sum::<usize>()
                },
            );
            self.present(frame, start, scene_time, vertices);
            return;
        }
        self.interact.scale = scale as f64;
        let marks = self.interact.marks(&self.engine, &self.viewport);
        let (viewport, text, lines) = (&self.viewport, &mut self.text, &mut self.lines);
        let scene = self
            .engine
            .with_plot_marks(viewport, scale, &marks, |plot| {
                build_scene_cached(viewport, scale, plot, text, lines)
            });
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
        let fractals = self
            .engine
            .fractals(origin, self.viewport.units_per_px, size);
        self.renderer.draw_scene(
            &self.gpu,
            &view,
            size,
            area,
            &scene,
            &self.text,
            &shadings,
            &fractals,
            &mut self.shader,
        );
        self.present(frame, start, scene_time, scene.vertices.len());
    }

    /// Shows a drawn frame and, now and then, its timings in the panel.
    fn present(
        &mut self,
        frame: wgpu::SurfaceTexture,
        start: Instant,
        scene_time: Duration,
        vertices: usize,
    ) {
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
                vertices
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
                } else if let Some(path) = self.open_on_start.take() {
                    self.open_path(path);
                } else {
                    self.restore();
                }
                self.update_title();
                self.window.request_redraw();
            }
            Some("select") => {
                let row = value["id"].as_u64();
                if self.engine.selected != row {
                    self.engine.selected = row;
                    self.interact.selection_changed();
                    self.window.request_redraw();
                }
            }
            Some("paste") => {
                let text = value["text"].as_str().unwrap_or_default().to_owned();
                self.paste(&text);
            }
            Some("mode") => {
                let three_d = value["mode"] == "3d";
                if three_d != self.three_d() {
                    self.set_mode(three_d);
                    if !self.smoke_test && self.autosave_due.is_none() {
                        self.autosave_due = Some(Instant::now() + AUTOSAVE_DELAY);
                    }
                }
            }
            Some("exportDesmos") => {
                let (state, warnings) = files::to_desmos(&self.rows, &self.viewport, &self.space());
                let reply = serde_json::json!({
                    "state": state.to_string(),
                    "threeD": self.three_d(),
                    "warnings": warnings,
                });
                self.run_script(&format!("window.engine.exported({reply})"));
            }
            Some(command @ ("new" | "open" | "save" | "saveAs")) => {
                let command = command.to_owned();
                self.file_command(&command);
            }
            Some("view") => self.set_view(value["view"] == "doc"),
            Some("docInit") => self.doc_init(),
            Some("docSave") => self.doc_save(&value),
            Some("docSaveAs") => self.doc_save_as(&value),
            Some("docOpen") => self.doc_open(value["path"].as_str().map(PathBuf::from)),
            Some("docRename") => self.doc_rename(&value),
            Some("docList") => self.doc_list(),
            Some("exportPdf") => self.export_pdf(&value),
            Some("shortcutsSave") => {
                if let Err(e) = docs::write_shortcuts(&value["list"]) {
                    self.doc_error(format!("Could not save the shortcuts: {e}"));
                }
            }
            Some("docNew") => {
                self.docs.path = None;
                self.remember_docs();
            }
            Some("solve") => {
                let id = value["id"].as_u64().unwrap_or(0);
                let latex = value["latex"].as_str().unwrap_or_default();
                let reply = crate::solve::reply(&self.rows, id, latex, value["op"].as_str());
                self.run_script(&format!("window.engine.solved({reply})"));
            }
            Some("rows") => {
                self.rows = value["rows"].clone();
                self.doc.rows_changed(&self.rows);
                self.update_title();
                if !self.smoke_test && self.autosave_due.is_none() {
                    self.autosave_due = Some(Instant::now() + AUTOSAVE_DELAY);
                }
                let results = self.engine.update(&value["rows"]);
                self.rows_received = true;
                self.run_script(&format!("window.engine.analysis({results})"));
                self.window.request_redraw();
            }
            _ => {}
        }
    }

    fn update_title(&self) {
        self.window.set_title(&self.doc.title());
    }

    /// New, open, save or save as, from the panel's menu or a shortcut.
    fn file_command(&mut self, command: &str) {
        match command {
            "new" => {
                if self.keep_changes() {
                    return;
                }
                self.doc.loading(None);
                let area = self.graph_area();
                self.viewport = Viewport::new(area.width as f64, area.height as f64);
                // A new graph keeps the mode, with the box as it starts.
                self.apply_space(Space {
                    three_d: self.three_d(),
                    view: None,
                });
                self.run_script("window.engine.load([])");
            }
            "open" => {
                if self.keep_changes() {
                    return;
                }
                if let Some(path) = dialogs::open(&self.window) {
                    self.open_path(path);
                }
            }
            "save" => match self.doc.path.clone() {
                Some(path) => self.save_to(path),
                None => self.file_command("saveAs"),
            },
            "saveAs" => {
                if let Some(path) = dialogs::save(&self.window, self.doc.path.as_deref()) {
                    self.save_to(files::with_extension(path));
                }
            }
            _ => {}
        }
        self.update_title();
        self.window.request_redraw();
    }

    /// When the graph has unsaved changes, asks whether to save them first.
    /// Returns true when the person cancelled, so nothing should happen.
    fn keep_changes(&mut self) -> bool {
        if !self.doc.dirty() {
            return false;
        }
        match dialogs::ask_to_save(&self.window, &self.doc.title()) {
            dialogs::Answer::Save => {
                self.file_command("save");
                self.doc.dirty()
            }
            dialogs::Answer::Discard => false,
            dialogs::Answer::Cancel => true,
        }
    }

    fn save_to(&mut self, path: PathBuf) {
        let text = files::to_file(&self.rows, &self.viewport, &self.space());
        match std::fs::write(&path, text) {
            Ok(()) => {
                self.doc.saved_to(path);
                self.autosave();
            }
            Err(e) => show_error(&format!("Could not save {}.\n\n{e}", path.display())),
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        let opened = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| Ok((files::open_any(&text)?, files::space_of(&text))));
        match opened {
            Ok(((rows, view, imported), space)) => {
                // An imported Desmos graph saves to a new graph file.
                self.doc.loading((!imported).then_some(path));
                if let Some(view) = view {
                    view.apply(&mut self.viewport);
                }
                self.apply_space(space);
                self.run_script(&format!("window.engine.load({rows})"));
            }
            Err(e) => show_error(&format!("Could not open {}.\n\n{e}", path.display())),
        }
    }

    /// A Desmos graph pasted into the panel: a desmos.com link, which is
    /// fetched, or the text `Calc.getState()` gives, which opens at once.
    fn paste(&mut self, text: &str) {
        if let Some(url) = files::desmos_link(text) {
            if self.keep_changes() {
                return;
            }
            self.run_script("window.engine.fetching(true)");
            let proxy = self.proxy.clone();
            std::thread::spawn(move || {
                let reply = fetch_desmos(&url);
                let _ = proxy.send_event(UserEvent::Fetched { url, reply });
            });
        } else if files::desmos_state(text).is_some() {
            if !self.keep_changes() {
                self.open_desmos(text, false);
            }
        } else {
            show_error(
                "That isn't a Desmos graph.\n\nPaste a desmos.com/calculator or desmos.com/3d link, or the text \
                 Calc.getState() gives in Desmos's browser console.",
            );
        }
    }

    fn fetched(&mut self, url: &str, reply: Result<String, String>) {
        self.run_script("window.engine.fetching(false)");
        match reply {
            Ok(body) if files::desmos_state(&body).is_some() => {
                self.open_desmos(&body, url.contains("desmos.com/3d/"))
            }
            Ok(_) => show_error(&format!("desmos.com did not send a graph for {url}.")),
            Err(e) => show_error(&format!("Could not get {url}.\n\n{e}")),
        }
    }

    /// Opens Desmos's state for a graph, in 3D when it is from Desmos 3D
    /// (or `three_d`, for a desmos.com/3d link).
    fn open_desmos(&mut self, text: &str, three_d: bool) {
        match files::open_any(text) {
            Ok((rows, view, _)) => {
                // Like an opened Desmos file, it saves to a new graph file.
                self.doc.loading(None);
                if let Some(view) = view {
                    view.apply(&mut self.viewport);
                }
                let mut space = files::space_of(text);
                space.three_d |= three_d;
                self.apply_space(space);
                self.run_script(&format!("window.engine.load({rows})"));
                self.update_title();
            }
            Err(e) => show_error(&format!("Could not open the Desmos graph.\n\n{e}")),
        }
    }

    /// Brings back the last session, unsaved changes included.
    fn restore(&mut self) {
        let Some(path) = files::autosave_path() else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let Some(restored) = files::read_autosave(&text) else {
            return;
        };
        restored.resume(&mut self.doc);
        if let Some(view) = restored.view {
            view.apply(&mut self.viewport);
        }
        self.apply_space(restored.space);
        self.run_script(&format!("window.engine.load({})", restored.rows));
    }

    fn autosave(&mut self) {
        self.autosave_due = None;
        if self.smoke_test || !self.rows_received {
            return;
        }
        let Some(path) = files::autosave_path() else {
            return;
        };
        let contents =
            files::autosave_contents(&self.doc, &self.rows, &self.viewport, &self.space());
        // Losing the autosave is not worth interrupting anyone over.
        let _ = files::write_autosave(&path, &contents);
    }

    /// Shows the Document view (the panel fills the window) or the graph.
    fn set_view(&mut self, doc_view: bool) {
        if doc_view == self.doc_view {
            return;
        }
        self.doc_view = doc_view;
        if let Some(webview) = &self.webview {
            let _ = webview.set_bounds(panel_bounds(&self.window, doc_view));
            if doc_view {
                let _ = webview.focus();
            }
        }
        self.docs.doc_view = doc_view;
        self.remember_docs();
        self.window.request_redraw();
    }

    fn library(&self) -> PathBuf {
        self.docs
            .library
            .clone()
            .unwrap_or_else(|| documents_dir().join(docs::LIBRARY_NAME))
    }

    fn remember_docs(&self) {
        if !self.smoke_test {
            docs::write_state(&self.docs);
        }
    }

    fn doc_reply(&self, function: &str, value: serde_json::Value) {
        self.run_script(&format!("window.doc.{function}({value})"));
    }

    fn doc_error(&self, message: String) {
        self.doc_reply("error", serde_json::Value::String(message));
    }

    /// The panel's document view starts: which document to show and where
    /// the library is. The view comes back as it was left.
    fn doc_init(&mut self) {
        let mut opened = serde_json::Value::Null;
        if let Some(path) = self.docs.path.clone() {
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|t| docs::from_file(&t))
            {
                Ok(doc) => opened = serde_json::json!({ "path": path, "doc": doc }),
                // Moved or deleted since: start with a new document.
                Err(_) => self.docs.path = None,
            }
        }
        let view = self.docs.doc_view && !self.smoke_test && self.open_on_start.is_none();
        self.doc_reply(
            "init",
            serde_json::json!({
                "library": self.library(),
                "opened": opened,
                "view": if view { "doc" } else { "graph" },
                "shortcuts": docs::read_shortcuts(),
                "pdf": self.docs.pdf,
            }),
        );
        self.doc_list();
    }

    /// Writes the document. A new one gets a file in the library named
    /// after its title.
    fn doc_save(&mut self, value: &serde_json::Value) {
        let title = value["doc"]["title"].as_str().unwrap_or_default();
        let path = match value["path"].as_str() {
            Some(p) => PathBuf::from(p),
            None => docs::free_path(&self.library(), title, None),
        };
        self.write_doc(path, &value["doc"]);
    }

    fn write_doc(&mut self, path: PathBuf, doc: &serde_json::Value) {
        let written = docs::to_file(doc).and_then(|text| docs::write(&path, &text));
        match written {
            Ok(()) => {
                let fresh = self.docs.path.as_ref() != Some(&path);
                self.docs.path = Some(path.clone());
                self.remember_docs();
                self.doc_reply("saved", serde_json::json!({ "path": path }));
                if fresh {
                    self.doc_list();
                }
            }
            Err(e) => self.doc_error(format!("Could not save {}: {e}", path.display())),
        }
    }

    fn doc_save_as(&mut self, value: &serde_json::Value) {
        let title = value["doc"]["title"].as_str().unwrap_or_default();
        let start = docs::free_path(&self.library(), title, None);
        if let Some(path) = dialogs::save_doc(&self.window, &start) {
            let path = path.with_extension(docs::EXTENSION);
            self.write_doc(path, &value["doc"]);
        }
    }

    /// Opens a document from the library, or one picked in the open dialog.
    fn doc_open(&mut self, path: Option<PathBuf>) {
        let Some(path) = path.or_else(|| dialogs::open_doc(&self.window, &self.library())) else {
            return;
        };
        match std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| docs::from_file(&t))
        {
            Ok(doc) => {
                self.docs.path = Some(path.clone());
                self.remember_docs();
                self.doc_reply("load", serde_json::json!({ "path": path, "doc": doc }));
            }
            Err(e) => self.doc_error(format!("Could not open {}: {e}", path.display())),
        }
    }

    /// Renames the document's file after its title changed.
    fn doc_rename(&mut self, value: &serde_json::Value) {
        let Some(old) = value["path"].as_str().map(PathBuf::from) else {
            return;
        };
        let Some(dir) = old.parent() else { return };
        let new = docs::free_path(dir, value["title"].as_str().unwrap_or_default(), Some(&old));
        if new == old {
            return;
        }
        match std::fs::rename(&old, &new) {
            Ok(()) => {
                self.docs.path = Some(new.clone());
                self.remember_docs();
                self.doc_reply("saved", serde_json::json!({ "path": new }));
                self.doc_list();
            }
            Err(e) => self.doc_error(format!("Could not rename {}: {e}", old.display())),
        }
    }

    /// Asks where to save, then has WebView2 print the document (with its
    /// print styles) to a PDF. The panel has already laid it out for print.
    fn export_pdf(&mut self, value: &serde_json::Value) {
        let options = &value["options"];
        self.docs.pdf = options.clone();
        self.remember_docs();
        let title = value["title"].as_str().unwrap_or_default();
        let start = match &self.docs.path {
            Some(doc) => doc.with_extension("pdf"),
            None => self
                .library()
                .join(format!("{}.pdf", docs::file_stem(title))),
        };
        let Some(path) = dialogs::save_pdf(&self.window, &start) else {
            self.doc_reply("pdfDone", serde_json::json!({ "cancelled": true }));
            return;
        };
        let path = path.with_extension("pdf");
        let letter = options["page"] != "a4";
        let header = options["header"] != false;
        let name = options["name"].as_str().unwrap_or_default().trim();
        let header_title = match (name.is_empty(), title.trim().is_empty()) {
            (false, false) => format!("{name} \u{b7} {}", title.trim()),
            (false, true) => name.to_owned(),
            _ => title.trim().to_owned(),
        };
        let settings = pdf::Settings {
            width: if letter { 8.5 } else { 8.27 },
            height: if letter { 11.0 } else { 11.69 },
            header,
            title: header_title,
        };
        let open = options["open"] != false;
        let proxy = self.proxy.clone();
        let done_path = path.clone();
        let started = match &self.webview {
            Some(webview) => pdf::print(webview, &path, &settings, move |result| {
                let _ = proxy.send_event(UserEvent::PdfSaved {
                    path: done_path,
                    result,
                    open,
                });
            }),
            None => Err("the panel is not running".into()),
        };
        if let Err(e) = started {
            self.pdf_saved(path, Err(e), false);
        }
    }

    fn pdf_saved(&mut self, path: PathBuf, result: Result<(), String>, open: bool) {
        match result {
            Ok(()) => {
                self.doc_reply("pdfDone", serde_json::json!({ "path": path }));
                if open {
                    open_file(&path);
                }
            }
            Err(e) => self.doc_reply(
                "pdfDone",
                serde_json::json!({ "error": format!("Could not save the PDF: {e}") }),
            ),
        }
    }

    fn doc_list(&self) {
        let library = self.library();
        let items = docs::list(&library);
        self.doc_reply(
            "library",
            serde_json::json!({ "folder": library, "items": items }),
        );
    }

    fn run_script(&self, js: &str) {
        if let Some(webview) = &self.webview {
            let _ = webview.evaluate_script(js);
        }
    }
}

/// The panel's place in the window: the left strip beside the graph, or
/// all of it in the Document view.
fn panel_bounds(window: &Window, doc_view: bool) -> Rect {
    let size = window.inner_size().to_logical::<f64>(window.scale_factor());
    let width = if doc_view { size.width } else { PANEL_WIDTH };
    Rect {
        position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
        size: wry::dpi::LogicalSize::new(width, size.height).into(),
    }
}

/// Opens a file in its usual app, as double-clicking it would.
fn open_file(path: &std::path::Path) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &std::ffi::OsStr| {
        use std::os::windows::ffi::OsStrExt;
        s.encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>()
    };
    let (verb, file) = (wide("open".as_ref()), wide(path.as_os_str()));
    // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call.
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

/// Printing the panel's page to a PDF with WebView2's own printer.
mod pdf {
    use std::path::Path;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        ICoreWebView2Environment6, ICoreWebView2_7, COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT,
    };
    use webview2_com::PrintToPdfCompletedHandler;
    use windows_core::{Interface, HSTRING};
    use wry::WebViewExtWindows;

    pub struct Settings {
        /// Page size in inches.
        pub width: f64,
        pub height: f64,
        /// Chromium's header (date, title) and footer (page numbers).
        pub header: bool,
        pub title: String,
    }

    /// Starts writing the PDF; `done` runs on the UI thread when it is
    /// written or has failed.
    pub fn print(
        webview: &wry::WebView,
        path: &Path,
        settings: &Settings,
        done: impl FnOnce(Result<(), String>) + 'static,
    ) -> Result<(), String> {
        let too_old = |_| {
            "this version of Microsoft Edge WebView2 cannot save PDFs; updating Windows updates it"
                .to_string()
        };
        let core: ICoreWebView2_7 = webview.webview().cast().map_err(too_old)?;
        let environment: ICoreWebView2Environment6 =
            webview.environment().cast().map_err(too_old)?;
        let failed = |e: windows_core::Error| e.message().to_string();
        // SAFETY: plain COM calls on live WebView2 objects, on the UI
        // thread that owns them; the strings outlive each call.
        unsafe {
            let print = environment.CreatePrintSettings().map_err(failed)?;
            print
                .SetOrientation(COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT)
                .map_err(failed)?;
            print.SetPageWidth(settings.width).map_err(failed)?;
            print.SetPageHeight(settings.height).map_err(failed)?;
            print.SetMarginTop(0.6).map_err(failed)?;
            print.SetMarginBottom(0.6).map_err(failed)?;
            print.SetMarginLeft(0.75).map_err(failed)?;
            print.SetMarginRight(0.75).map_err(failed)?;
            print.SetShouldPrintBackgrounds(true).map_err(failed)?;
            print
                .SetShouldPrintHeaderAndFooter(settings.header)
                .map_err(failed)?;
            if settings.header {
                print
                    .SetHeaderTitle(&HSTRING::from(settings.title.as_str()))
                    .map_err(failed)?;
                // No address in the footer, just the page numbers.
                print.SetFooterUri(&HSTRING::new()).map_err(failed)?;
            }
            let handler = PrintToPdfCompletedHandler::create(Box::new(
                move |status: windows_core::Result<()>, ok: bool| {
                    done(match status {
                        Ok(()) if ok => Ok(()),
                        Ok(()) => Err("WebView2 could not write the file".into()),
                        Err(e) => Err(e.message().to_string()),
                    });
                    Ok(())
                },
            ));
            core.PrintToPdf(&HSTRING::from(path.as_os_str()), &print, &handler)
                .map_err(failed)
        }
    }
}

/// The person's Documents folder, wherever Windows keeps it (OneDrive moves
/// it, for one).
fn documents_dir() -> PathBuf {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath};
    let mut raw: *mut u16 = std::ptr::null_mut();
    // SAFETY: the out pointer is valid; the string Windows returns is
    // NUL-terminated and freed with CoTaskMemFree once copied.
    let found = unsafe {
        let ok = SHGetKnownFolderPath(&FOLDERID_Documents, 0, std::ptr::null_mut(), &mut raw) == 0;
        let path = (ok && !raw.is_null()).then(|| {
            let len = (0..).take_while(|&i| *raw.add(i) != 0).count();
            String::from_utf16_lossy(std::slice::from_raw_parts(raw, len))
        });
        if !raw.is_null() {
            CoTaskMemFree(raw as *const core::ffi::c_void);
        }
        path
    };
    found.map(PathBuf::from).unwrap_or_else(|| {
        std::env::var_os("USERPROFILE")
            .map(|h| PathBuf::from(h).join("Documents"))
            .unwrap_or_else(|| PathBuf::from("."))
    })
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
                s.panel_ready && s.frames > 0 && s.engine.drawn() > 0,
                format!(
                    "adapter: {}\nframes rendered: {}\npanel ready: {}\nrows from panel: {:?}\n\
                     curves drawn: {}\nframes per second while a slider plays: {fps:.1}\n",
                    s.gpu.describe(),
                    s.frames,
                    s.panel_ready,
                    s.engine.latex,
                    s.engine.drawn(),
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

/// Asks desmos.com for a saved graph. With `Accept: application/json` it
/// answers with the graph's state instead of the calculator page.
fn fetch_desmos(url: &str) -> Result<String, String> {
    let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
    let agent = ureq::AgentBuilder::new()
        .tls_connector(Arc::new(tls))
        .timeout(Duration::from_secs(20))
        .build();
    let reply = agent
        .get(url)
        .set("Accept", "application/json")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => "desmos.com has no graph at that link.".to_string(),
            e => e.to_string(),
        })?;
    reply.into_string().map_err(|e| e.to_string())
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

/// Windows' own file and question dialogs, owned by the calculator window.
mod dialogs {
    use super::files::EXTENSION;
    use std::path::{Path, PathBuf};
    use windows_sys::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, GetSaveFileNameW, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR,
        OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDCANCEL, IDNO, MB_ICONWARNING, MB_YESNOCANCEL,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn hwnd(window: &Window) -> *mut core::ffi::c_void {
        match window.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(h)) => h.hwnd.get() as *mut core::ffi::c_void,
            _ => std::ptr::null_mut(),
        }
    }

    /// Runs the open or save dialog, starting from `initial` when given.
    fn file_dialog(window: &Window, save: bool, initial: Option<&Path>) -> Option<PathBuf> {
        // Filters are pairs of NUL-terminated strings, ended by an extra NUL.
        // Opening also takes Desmos graph states saved as JSON or text.
        let desmos = if save {
            ""
        } else {
            "Desmos graph states (*.json, *.txt)\0*.json;*.txt\0"
        };
        let filter =
            format!("Graphs (*.{EXTENSION})\0*.{EXTENSION}\0{desmos}All files (*.*)\0*.*\0\0");
        run_dialog(window, save, initial, None, &filter, EXTENSION)
    }

    fn run_dialog(
        window: &Window,
        save: bool,
        initial: Option<&Path>,
        folder: Option<&Path>,
        filter: &str,
        extension: &str,
    ) -> Option<PathBuf> {
        let filter: Vec<u16> = filter.encode_utf16().collect();
        let ext = wide(extension);
        let folder = folder.and_then(|f| f.to_str()).map(wide);
        let mut buffer = vec![0u16; 4096];
        if let Some(name) = initial.and_then(|p| p.to_str()) {
            let name: Vec<u16> = name.encode_utf16().take(buffer.len() - 1).collect();
            buffer[..name.len()].copy_from_slice(&name);
        }
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: hwnd(window),
            lpstrFilter: filter.as_ptr(),
            lpstrFile: buffer.as_mut_ptr(),
            nMaxFile: buffer.len() as u32,
            lpstrDefExt: ext.as_ptr(),
            lpstrInitialDir: folder.as_ref().map_or(std::ptr::null(), |f| f.as_ptr()),
            Flags: OFN_EXPLORER
                | OFN_NOCHANGEDIR
                | OFN_PATHMUSTEXIST
                | if save {
                    OFN_OVERWRITEPROMPT
                } else {
                    OFN_FILEMUSTEXIST
                },
            ..Default::default()
        };
        // SAFETY: every pointer in `ofn` refers to a buffer that outlives
        // the call, and the file buffer's length is passed with it.
        let ok = unsafe {
            if save {
                GetSaveFileNameW(&mut ofn)
            } else {
                GetOpenFileNameW(&mut ofn)
            }
        };
        if ok == 0 {
            return None;
        }
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        Some(PathBuf::from(String::from_utf16_lossy(&buffer[..len])))
    }

    pub fn open(window: &Window) -> Option<PathBuf> {
        file_dialog(window, false, None)
    }

    pub fn save(window: &Window, current: Option<&Path>) -> Option<PathBuf> {
        file_dialog(window, true, current)
    }

    const DOC_FILTER: &str = "Math documents (*.gcdoc)\0*.gcdoc\0All files (*.*)\0*.*\0\0";

    /// Opens a math document, starting in the library folder.
    pub fn open_doc(window: &Window, library: &Path) -> Option<PathBuf> {
        let _ = std::fs::create_dir_all(library);
        run_dialog(window, false, None, Some(library), DOC_FILTER, "gcdoc")
    }

    /// Where to save a PDF, suggesting `start`.
    pub fn save_pdf(window: &Window, start: &Path) -> Option<PathBuf> {
        if let Some(dir) = start.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let filter = "PDF documents (*.pdf)\0*.pdf\0All files (*.*)\0*.*\0\0";
        run_dialog(window, true, Some(start), None, filter, "pdf")
    }

    /// Saves a math document somewhere new, suggesting `start`.
    pub fn save_doc(window: &Window, start: &Path) -> Option<PathBuf> {
        if let Some(dir) = start.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        run_dialog(window, true, Some(start), None, DOC_FILTER, "gcdoc")
    }

    pub enum Answer {
        Save,
        Discard,
        Cancel,
    }

    /// "Save changes to this graph?" with Yes, No and Cancel.
    pub fn ask_to_save(window: &Window, title: &str) -> Answer {
        let name = title
            .trim_start_matches('*')
            .split(" - ")
            .next()
            .unwrap_or("this graph");
        let text = wide(&format!("Save changes to {name}?"));
        let caption = wide("Graphing Calculator");
        // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call.
        let answer = unsafe {
            MessageBoxW(
                hwnd(window),
                text.as_ptr(),
                caption.as_ptr(),
                MB_YESNOCANCEL | MB_ICONWARNING,
            )
        };
        match answer {
            IDCANCEL => Answer::Cancel,
            IDNO => Answer::Discard,
            _ => Answer::Save,
        }
    }
}
