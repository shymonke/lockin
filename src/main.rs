#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod autostart;
mod ball;
mod calc;
mod camera;
mod config;
mod copy;
mod expr;
mod fold;
mod feedback;
mod hotkeys;
mod memes;
mod policy;
mod pomodoro;
mod screen;
mod theme;
mod tasks;
mod tray;
mod ui;
mod vision;
mod web;
mod window_frame;

use eframe::egui;

impl eframe::App for app::App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tick(ctx);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.demo_backdrop(&ctx);
        self.pill_ui(ui);
        self.alert_window(&ctx);
        self.calc_window(&ctx);
        window_frame::remove_overlay_frames();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--diag") => return diag(&args[2..]),
        Some("--diag-screen") => return screen::diag(),
        _ => {}
    }
    if !single_instance() {
        return;
    }
    // Release builds have no console, so a panic would vanish with the process: keep it in
    // %APPDATA%\LockIn\crash.log, then let the default hook run.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let line = format!("[{secs}] v{} {info}\n", env!("CARGO_PKG_VERSION"));
        let path = config::data_dir().join("crash.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = std::io::Write::write_all(&mut f, line.as_bytes());
        }
        default_hook(info);
    }));
    // `--demo [marks.json]`: scripted showcase on a throwaway data folder (see app/demo.rs).
    let demo = (args.get(1).map(String::as_str) == Some("--demo")).then(|| args.get(2).map(std::path::PathBuf::from));
    let s = match &demo {
        Some(_) => app::demo::prepare(&config::Settings::load()),
        None => config::Settings::load(),
    };
    let icon = image::load_from_memory(include_bytes!("../assets/brand/png/app-icon-64.png"))
        .expect("embedded LockIn window icon")
        .into_rgba8();
    let (icon_width, icon_height) = icon.dimensions();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("LockIn")
        .with_icon(egui::IconData { rgba: icon.into_raw(), width: icon_width, height: icon_height })
        .with_inner_size(ui::PILL)
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_taskbar(false)
        .with_resizable(false)
        .with_mouse_passthrough(s.overlay_click_through);
    if let Some([x, y]) = s.overlay_pos {
        viewport = viewport.with_position([x, y]);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    let _ = eframe::run_native(
        "LockIn",
        options,
        Box::new(move |cc| {
            let mut app = app::App::new(cc, s);
            if let Some(marks) = demo {
                app.demo_seed();
                app.demo = Some(app::demo::Demo::new(marks));
            }
            Ok(Box::new(app))
        }),
    );
}

/// Set on the instance spawned by "Restart LockIn".
pub const RESTART_ENV: &str = "LOCKIN_RESTART";

/// One overlay at a time (hotkeys and the camera are exclusive anyway). A restarted instance
/// waits for the old one to exit (its mutex is then abandoned and passes to us).
fn single_instance() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{CreateMutexW, WaitForSingleObject};
    let wait_ms = if std::env::var_os(RESTART_ENV).is_some() { 15_000 } else { 0 };
    // Children (WebView2) need not see it.
    std::env::remove_var(RESTART_ENV);
    // SAFETY: named mutex lives for the process lifetime (handle intentionally leaked).
    unsafe {
        let Ok(m) = CreateMutexW(None, false, w!("Local\\LockIn.SingleInstance")) else { return false };
        matches!(WaitForSingleObject(m, wait_ms), WAIT_OBJECT_0 | WAIT_ABANDONED)
    }
}

/// `lockin --diag [image ...]`: run the models on images (or the live camera when none given)
/// and print detections, poses and timings. Live, it first prints the camera's warm-up frames,
/// then times each stage over 60 frames at the preview's ~12 fps.
fn diag(images: &[String]) {
    use std::time::{Duration, Instant};
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1e3;
    let settings = config::Settings::load();
    let (calibration, gaze_mode) = (settings.calibration, settings.gaze_mode);
    let t = Instant::now();
    let face = vision::FaceModel::load().expect("face model");
    println!("face model load {:.0} ms", ms(t));
    let t = Instant::now();
    let mesh = vision::MeshModel::load().expect("face mesh model");
    println!("face mesh model load {:.0} ms", ms(t));
    let t = Instant::now();
    let obj = vision::ObjectModel::load().expect("object model");
    println!("object model load {:.0} ms", ms(t));
    let report = |f: &camera::Frame| {
        let t = Instant::now();
        let d = face.detect(f).unwrap();
        let m = d.as_ref().and_then(|d| mesh.detect(f, d, true).unwrap());
        let l1 = ms(t);
        let t = Instant::now();
        let o = obj.detect(f).unwrap();
        let l2 = ms(t);
        match d {
            Some(d) => {
                let gaze = m.as_ref().map(|m| m.direction());
                let overlays = m.as_ref().map(|m| m.overlays()).unwrap_or_default();
                let feature = m.as_ref().map(|m| vision::feature_sample(&d, m));
                let classified = feature
                    .as_ref()
                    .and_then(|s| policy::classify_gaze_features(s, &calibration, gaze_mode))
                    .unwrap_or((policy::classify_gaze(d.pose(), &calibration), 0.45));
                println!(
                    "  face {:.2} rect {:?} head {:?} gaze {:?} look {:?} label {:?} confidence {:.2} | person {:.2} phone {:.2} | L1+mesh {l1:.1} ms L2 {l2:.1} ms",
                    d.score,
                    d.rect.map(|v| (v * 100.0).round() / 100.0),
                    m.as_ref().map(|m| m.head().map(|v| (v * 10.0).round() / 10.0)),
                    gaze.map(|g| g.map(|v| (v * 100.0).round() / 100.0)),
                    feature.as_ref().map(|s| [s.values[12], s.values[13]].map(|v| (v * 100.0).round() / 100.0)),
                    classified.0,
                    classified.1,
                    o.person,
                    o.phone
                );
                for (i, e) in overlays.iter().enumerate() {
                    println!("    eye {i}: iris [{:.4}, {:.4}] r {:.4} open {:.3} contour {}", e.iris[0], e.iris[1], e.iris_r, e.open, e.contour.len());
                }
            }
            None => println!(
                "  no face, label Absent confidence 1.00 | person {:.2} phone {:.2} | L1+mesh {l1:.1} ms L2 {l2:.1} ms",
                o.person, o.phone
            ),
        }
    };
    // Stage timings at the preview cadence: YuNet, mesh alone, mesh plus blendshapes, and the
    // tracked path the worker takes between detections (mesh seeded from the last frame's mesh).
    let timings = |grab: &mut dyn FnMut(&mut camera::Frame)| {
        let mut f = camera::Frame { width: 0, height: 0, bgra: Vec::new() };
        let mut stages: [(&str, Vec<f64>); 6] = [
            ("grab (wait for frame)", Vec::new()),
            ("YuNet face box", Vec::new()),
            ("mesh only (Gaze only)", Vec::new()),
            ("mesh + blendshapes", Vec::new()),
            ("tracked check, Gaze only", Vec::new()),
            ("tracked check, Gaze, head and position", Vec::new()),
        ];
        let mut seed: Option<vision::Face> = None;
        let started = Instant::now();
        for i in 0..60u32 {
            std::thread::sleep((started + Duration::from_secs_f64(i as f64 / 12.0)).saturating_duration_since(Instant::now()));
            let t = Instant::now();
            grab(&mut f);
            stages[0].1.push(ms(t));
            let t = Instant::now();
            let Some(d) = face.detect(&f).unwrap() else { continue };
            stages[1].1.push(ms(t));
            let t = Instant::now();
            let _ = mesh.detect(&f, &d, false).unwrap();
            stages[2].1.push(ms(t));
            let t = Instant::now();
            let full = mesh.detect(&f, &d, true).unwrap();
            stages[3].1.push(ms(t));
            if let Some(s) = seed {
                let t = Instant::now();
                let _ = mesh.detect(&f, &s, false).unwrap();
                stages[4].1.push(ms(t));
                let t = Instant::now();
                let _ = mesh.detect(&f, &s, true).unwrap();
                stages[5].1.push(ms(t));
            }
            seed = full.map(|m| m.face_box(d.score, f.width as f32 / f.height as f32));
        }
        println!("{:<40} {:>3} {:>6} {:>6} {:>6} {:>6} {:>6}", "stage (ms)", "n", "min", "median", "p95", "max", "sd");
        for (name, v) in &mut stages {
            if v.is_empty() {
                println!("{name:<40}   0 (no face found)");
                continue;
            }
            v.sort_by(f64::total_cmp);
            let n = v.len();
            let mean = v.iter().sum::<f64>() / n as f64;
            let sd = (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64).sqrt();
            let q = |p: f64| v[((n - 1) as f64 * p).round() as usize];
            println!("{name:<40} {n:>3} {:>6.1} {:>6.1} {:>6.1} {:>6.1} {:>6.1}", v[0], q(0.5), q(0.95), v[n - 1], sd);
        }
    };
    if images.is_empty() {
        let _ = camera::startup();
        println!("cameras: {:?}", camera::list());
        let opened = Instant::now();
        let mut cam = camera::Camera::open(0).expect("open camera");
        let mut f = camera::Frame { width: 0, height: 0, bgra: Vec::new() };
        let luma = |f: &camera::Frame| f.bgra.chunks_exact(4).map(|p| p[1] as u64).sum::<u64>() / (f.width * f.height).max(1) as u64;
        println!("warm-up (ms since open: mean brightness/255):");
        let mut line = String::new();
        while opened.elapsed() < Duration::from_millis(1500) {
            cam.grab(&mut f).expect("grab");
            line += &format!(" {:.0}:{}", ms(opened), luma(&f));
        }
        println!(" {line}");
        report(&f);
        timings(&mut |f| cam.grab(f).expect("grab"));
    }
    let mut last = None;
    for path in images {
        let img = image::open(path).expect("open image").to_rgba8();
        let (w, h) = img.dimensions();
        let mut bgra = img.into_raw();
        for p in bgra.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        println!("{path} {w}x{h}");
        let f = camera::Frame { width: w as usize, height: h as usize, bgra };
        report(&f);
        last = Some(f);
    }
    // Images: the same timings on the last image, shown 60 times.
    if let Some(img) = last {
        timings(&mut |f| {
            f.width = img.width;
            f.height = img.height;
            f.bgra.clone_from(&img.bgra);
        });
    }
}
