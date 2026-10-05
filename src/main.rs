#[cfg(feature = "scripting-lua")]
use libgpteng::ScriptWorker;
use libgpteng::{Engine, EngineConfig, Material3d, MeshKind, PointLight3d, Transform};
#[cfg(feature = "desktop-window")]
use libgpteng::{RenderCamera, SceneRenderer};
#[cfg(feature = "desktop-window")]
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
#[cfg(feature = "desktop-window")]
use winit::keyboard::KeyCode;

#[cfg(feature = "mcp")]
use libgpteng::mcp::{EngineMcpServer, serve_http, serve_stdio};

#[derive(Debug, Default)]
struct LaunchOptions {
    desktop: bool,
    desktop_explicit: bool,
    mcp_stdio: bool,
    mcp_http: Option<SocketAddr>,
    webmcp: bool,
    show_help: bool,
    show_version: bool,
}

fn parse_args() -> Result<LaunchOptions, String> {
    let mut options = LaunchOptions::default();
    let mut args = std::env::args().skip(1).peekable();
    let mut server_mode = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--desktop" => {
                options.desktop = true;
                options.desktop_explicit = true;
            }
            "--no-desktop" => {
                options.desktop = false;
                options.desktop_explicit = true;
            }
            "--mcp-stdio" => {
                options.mcp_stdio = true;
                server_mode = true;
            }
            "--mcp-http" => {
                options.mcp_http =
                    Some(match args.peek().filter(|value| !value.starts_with('-')) {
                        Some(_) => args
                            .next()
                            .expect("peeked argument exists")
                            .parse()
                            .map_err(|e| format!("invalid HTTP bind address: {e}"))?,
                        None => "127.0.0.1:8000".parse().expect("default address is valid"),
                    });
                server_mode = true;
            }
            "--webmcp" => {
                options.webmcp = true;
                options.mcp_http.get_or_insert_with(|| {
                    "127.0.0.1:8000".parse().expect("default address is valid")
                });
                server_mode = true;
            }
            "--help" | "-h" => options.show_help = true,
            "--version" | "-V" => options.show_version = true,
            unknown => return Err(format!("unknown argument: {unknown}")),
        }
    }

    if server_mode && !options.desktop_explicit {
        options.desktop = false;
    }
    if !server_mode && !options.desktop_explicit {
        options.desktop = true;
    }
    Ok(options)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let options =
        parse_args().map_err(|message| format!("{message}\nRun `gpteng --help` for usage."))?;
    if options.show_help {
        print_help();
        return Ok(());
    }
    if options.show_version {
        println!(
            "gpteng {} / libgpteng {}",
            env!("CARGO_PKG_VERSION"),
            libgpteng::VERSION
        );
        return Ok(());
    }

    let mut initial_engine = Engine::new(EngineConfig::default())?;
    let face_texture = initial_engine.add_texture_asset(
        2,
        2,
        vec![
            235, 133, 72, 255, 224, 117, 63, 255, 242, 143, 79, 255, 231, 126, 68, 255,
        ],
    )?;
    let demo_head = initial_engine.spawn_donald_trump_caricature(glam::Vec3::ZERO)?;
    initial_engine.set_base_color_texture(demo_head.face, Some(face_texture))?;
    let translucent_orb = initial_engine.spawn_mesh(
        "Translucent demo orb",
        Transform {
            translation: glam::Vec3::new(-1.05, 0.48, 0.18),
            scale: glam::Vec3::splat(0.42),
            ..Transform::default()
        },
        MeshKind::Sphere,
        Material3d {
            base_color: [0.12, 0.72, 0.98, 0.38],
            roughness: 0.12,
            metallic: 0.15,
            ..Material3d::default()
        },
    )?;
    initial_engine.set_parent(translucent_orb, Some(demo_head.root))?;
    initial_engine.set_point_light(
        translucent_orb,
        Some(PointLight3d {
            color: [0.3, 0.68, 1.0],
            intensity: 1.4,
            radius: 3.0,
        }),
    )?;
    let _engine = Arc::new(Mutex::new(initial_engine));
    #[cfg(feature = "scripting-lua")]
    let scripts = ScriptWorker::new(Arc::clone(&_engine));
    #[cfg(feature = "scripting-lua")]
    scripts.attach(
        demo_head.root.get(),
        "local id = engine.self()\nlocal elapsed = 0\nfunction update(dt)\n  elapsed = elapsed + dt\n  local yaw = math.sin(elapsed * 0.7) * 0.16\n  local pitch = math.sin(elapsed * 1.1) * 0.035\n  local sx = math.sin(pitch * 0.5)\n  local cy = math.cos(yaw * 0.5)\n  local cx = math.cos(pitch * 0.5)\n  local sy = math.sin(yaw * 0.5)\n  engine.set_rotation(id, sx * cy, cx * sy, -sx * sy, cx * cy)\nend",
    )
    .map_err(std::io::Error::other)?;
    #[cfg(all(feature = "mcp", feature = "scripting-lua"))]
    let server =
        EngineMcpServer::new_shared_with_script_worker(Arc::clone(&_engine), scripts.clone());
    #[cfg(all(feature = "mcp", not(feature = "scripting-lua")))]
    let server = EngineMcpServer::new_shared(Arc::clone(&_engine));

    #[cfg(not(feature = "mcp"))]
    if options.mcp_stdio || options.mcp_http.is_some() || options.webmcp {
        return Err("MCP support is disabled; enable the `mcp` feature.".into());
    }

    #[cfg(feature = "mcp")]
    let mut servers = tokio::task::JoinSet::new();

    #[cfg(feature = "mcp")]
    if options.mcp_stdio {
        let stdio_server = server.clone();
        servers.spawn(async move {
            let result = serve_stdio(stdio_server).await;
            if let Err(error) = &result {
                eprintln!("MCP stdio server stopped: {error}");
            }
            result
        });
    }

    #[cfg(feature = "mcp")]
    if let Some(bind) = options.mcp_http {
        let http_server = server.clone();
        let webmcp = options.webmcp;
        servers.spawn(async move {
            let result = serve_http(http_server, bind, webmcp).await;
            if let Err(error) = &result {
                eprintln!("MCP HTTP server stopped: {error}");
            }
            result
        });
        eprintln!("MCP/WebMCP HTTP listener requested at http://{bind}");
        if webmcp {
            eprintln!("WebMCP page: http://{bind}/webmcp");
        }
    }

    if options.desktop {
        #[cfg(feature = "desktop-window")]
        {
            run_desktop(_engine)?;
            return Ok(());
        }
        #[cfg(not(feature = "desktop-window"))]
        {
            return Err("desktop mode is unavailable; enable the `desktop-window` feature.".into());
        }
    }

    #[cfg(feature = "mcp")]
    while let Some(result) = servers.join_next().await {
        result??;
    }
    Ok(())
}

#[cfg(feature = "desktop-window")]
#[allow(deprecated)]
fn run_desktop(engine: Arc<Mutex<Engine>>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use glam::Vec3;
    use winit::{
        event::{DeviceEvent, ElementState, Event, MouseButton, WindowEvent},
        event_loop::{ControlFlow, EventLoop},
        keyboard::{KeyCode, PhysicalKey},
        window::{CursorGrabMode, Window},
    };

    let event_loop = EventLoop::new()?;
    let window = Arc::new(
        event_loop
            .create_window(Window::default_attributes().with_title("gpteng — Rust 3D Engine"))?,
    );
    window.set_visible(true);

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_with_display_handle(
        Box::new(event_loop.owned_display_handle()),
    ));
    let surface = instance.create_surface(Arc::clone(&window))?;
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: Some(&surface),
        apply_limit_buckets: false,
    }))?;
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
    let size = window.inner_size();
    let mut surface_config = surface
        .get_default_config(&adapter, size.width.max(1), size.height.max(1))
        .ok_or("the graphics adapter cannot present to this window")?;
    surface.configure(&device, &surface_config);
    let mut scene_renderer = SceneRenderer::new(
        &device,
        &queue,
        surface_config.format,
        surface_config.width,
        surface_config.height,
    );
    let mut pressed_keys = HashSet::<KeyCode>::new();
    let mut mouse_look_enabled = false;
    let mut last_frame = Instant::now();

    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run(move |event, target| match event {
        Event::WindowEvent { window_id, event } if window_id == window.id() => match event {
            WindowEvent::CloseRequested => target.exit(),
            WindowEvent::Focused(false) => {
                let mut engine = engine
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for key in &pressed_keys {
                    let _ = engine.set_key_down(&desktop_key_name(*key), false);
                }
                pressed_keys.clear();
                for button in ["right", "left", "middle"] {
                    let _ = engine.set_mouse_button_down(button, false);
                }
                if mouse_look_enabled {
                    let _ = window.set_cursor_grab(CursorGrabMode::None);
                    window.set_cursor_visible(true);
                    mouse_look_enabled = false;
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button_name = match button {
                    MouseButton::Left => Some("left"),
                    MouseButton::Right => Some("right"),
                    MouseButton::Middle => Some("middle"),
                    MouseButton::Back => Some("back"),
                    MouseButton::Forward => Some("forward"),
                    MouseButton::Other(_) => None,
                };
                if let Some(button_name) = button_name {
                    let mut engine = engine
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let _ =
                        engine.set_mouse_button_down(button_name, state == ElementState::Pressed);
                }
                if button == MouseButton::Right {
                    if state == ElementState::Pressed && !mouse_look_enabled {
                        let grab = window
                            .set_cursor_grab(CursorGrabMode::Locked)
                            .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
                        if grab.is_ok() {
                            mouse_look_enabled = true;
                            window.set_cursor_visible(false);
                        }
                    } else if state == ElementState::Released && mouse_look_enabled {
                        let _ = window.set_cursor_grab(CursorGrabMode::None);
                        window.set_cursor_visible(true);
                        mouse_look_enabled = false;
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    if key == KeyCode::Escape && event.state == ElementState::Pressed {
                        target.exit();
                    }
                    match event.state {
                        ElementState::Pressed => {
                            pressed_keys.insert(key);
                        }
                        ElementState::Released => {
                            pressed_keys.remove(&key);
                        }
                    }
                    let mut engine = engine
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let _ = engine
                        .set_key_down(&desktop_key_name(key), event.state == ElementState::Pressed);
                }
            }
            WindowEvent::RedrawRequested => {
                let engine = engine
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let frame = match surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(frame)
                    | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
                    wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                        surface.configure(&device, &surface_config);
                        return;
                    }
                    wgpu::CurrentSurfaceTexture::Timeout
                    | wgpu::CurrentSurfaceTexture::Occluded
                    | wgpu::CurrentSurfaceTexture::Validation => return,
                };
                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());
                let camera = RenderCamera::from(engine.camera());
                scene_renderer.render(&engine, camera, &device, &queue, &view);
                queue.present(frame);
            }
            WindowEvent::Resized(size) if size.width > 0 && size.height > 0 => {
                surface_config.width = size.width;
                surface_config.height = size.height;
                surface.configure(&device, &surface_config);
                scene_renderer.resize(&device, size.width, size.height);
                window.request_redraw();
            }
            _ => {}
        },
        Event::DeviceEvent {
            event: DeviceEvent::MouseMotion { delta },
            ..
        } if mouse_look_enabled => {
            let mut engine = engine
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = engine.add_mouse_motion(delta.0, delta.1);
            let mut camera = engine.camera();
            let direction = (camera.target - camera.position).normalize_or_zero();
            let yaw = direction.x.atan2(direction.z) - delta.0 as f32 * 0.0025;
            let pitch = (direction.y.asin() - delta.1 as f32 * 0.0025).clamp(-1.54, 1.54);
            let horizontal = pitch.cos();
            let direction = Vec3::new(yaw.sin() * horizontal, pitch.sin(), yaw.cos() * horizontal);
            camera.target = camera.position + direction;
            let _ = engine.set_camera(camera);
        }
        Event::AboutToWait => {
            let now = Instant::now();
            let delta_seconds = now.duration_since(last_frame).as_secs_f32().min(0.1);
            last_frame = now;

            let mut engine = engine
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut camera = engine.camera();
            let forward = (camera.target - camera.position).normalize_or_zero();
            let right = forward.cross(camera.up).normalize_or_zero();
            let mut movement = Vec3::ZERO;
            if pressed_keys.contains(&KeyCode::KeyW) {
                movement += forward;
            }
            if pressed_keys.contains(&KeyCode::KeyS) {
                movement -= forward;
            }
            if pressed_keys.contains(&KeyCode::KeyD) {
                movement += right;
            }
            if pressed_keys.contains(&KeyCode::KeyA) {
                movement -= right;
            }
            if pressed_keys.contains(&KeyCode::KeyE) {
                movement += camera.up;
            }
            if pressed_keys.contains(&KeyCode::KeyQ) {
                movement -= camera.up;
            }
            if movement != Vec3::ZERO {
                let speed = if pressed_keys.contains(&KeyCode::ShiftLeft)
                    || pressed_keys.contains(&KeyCode::ShiftRight)
                {
                    9.0
                } else {
                    3.0
                };
                let offset = movement.normalize() * speed * delta_seconds;
                camera.position += offset;
                camera.target += offset;
                let _ = engine.set_camera(camera);
            }

            window.request_redraw();
            target.set_control_flow(ControlFlow::WaitUntil(now + Duration::from_millis(16)));
        }
        _ => {}
    })?;
    Ok(())
}

#[cfg(feature = "desktop-window")]
fn desktop_key_name(key: KeyCode) -> String {
    match key {
        KeyCode::KeyA => "a".into(),
        KeyCode::KeyB => "b".into(),
        KeyCode::KeyC => "c".into(),
        KeyCode::KeyD => "d".into(),
        KeyCode::KeyE => "e".into(),
        KeyCode::KeyF => "f".into(),
        KeyCode::KeyG => "g".into(),
        KeyCode::KeyH => "h".into(),
        KeyCode::KeyI => "i".into(),
        KeyCode::KeyJ => "j".into(),
        KeyCode::KeyK => "k".into(),
        KeyCode::KeyL => "l".into(),
        KeyCode::KeyM => "m".into(),
        KeyCode::KeyN => "n".into(),
        KeyCode::KeyO => "o".into(),
        KeyCode::KeyP => "p".into(),
        KeyCode::KeyQ => "q".into(),
        KeyCode::KeyR => "r".into(),
        KeyCode::KeyS => "s".into(),
        KeyCode::KeyT => "t".into(),
        KeyCode::KeyU => "u".into(),
        KeyCode::KeyV => "v".into(),
        KeyCode::KeyW => "w".into(),
        KeyCode::KeyX => "x".into(),
        KeyCode::KeyY => "y".into(),
        KeyCode::KeyZ => "z".into(),
        KeyCode::Digit0 => "0".into(),
        KeyCode::Digit1 => "1".into(),
        KeyCode::Digit2 => "2".into(),
        KeyCode::Digit3 => "3".into(),
        KeyCode::Digit4 => "4".into(),
        KeyCode::Digit5 => "5".into(),
        KeyCode::Digit6 => "6".into(),
        KeyCode::Digit7 => "7".into(),
        KeyCode::Digit8 => "8".into(),
        KeyCode::Digit9 => "9".into(),
        KeyCode::Space => "space".into(),
        KeyCode::Escape => "escape".into(),
        KeyCode::ShiftLeft => "shift_left".into(),
        KeyCode::ShiftRight => "shift_right".into(),
        other => format!("{other:?}").to_ascii_lowercase(),
    }
}

fn print_help() {
    println!(
        "gpteng — desktop client and engine host\n\nUsage:\n  gpteng [--desktop] [server options]\n\nModes (combinable):\n  --desktop          Open the desktop client\n  --no-desktop       Do not open the desktop client\n  --mcp-stdio        Run the MCP server on stdin/stdout\n  --mcp-http [ADDR]  Run Streamable HTTP MCP (default 127.0.0.1:8000)\n  --webmcp           Serve the browser WebMCP page at /webmcp (implies HTTP)\n\nWith no options, the desktop client opens. Adding server options disables the\ndesktop client unless --desktop is also specified. HTTP and WebMCP share one\nlistener and one set of engine tools."
    );
}
