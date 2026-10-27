//! The calculator window: wgpu draws the graph into the window itself, and a
//! WebView2 child covers the left side with the MathQuill expression panel.
//! This is the layout wry's own wgpu example uses on Windows.

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
                match c.to_lowercase().as_str() {
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
            .with_bounds(panel_bounds(&window))
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
            let _ = webview.set_bounds(panel_bounds(&self.window));
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
        let scene = self.engine.with_plot_marks(&marks, |plot| {
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
            Some(command @ ("new" | "open" | "save" | "saveAs")) => {
                let command = command.to_owned();
                self.file_command(&command);
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
        let filter: Vec<u16> =
            format!("Graphs (*.{EXTENSION})\0*.{EXTENSION}\0{desmos}All files (*.*)\0*.*\0\0")
                .encode_utf16()
                .collect();
        let ext = wide(EXTENSION);
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
