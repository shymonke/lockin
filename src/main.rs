#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod calc;
mod camera;
mod config;
mod copy;
mod expr;
mod hotkeys;
mod memes;
mod policy;
mod pomodoro;
mod screen;
mod theme;
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
    let s = config::Settings::load();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("LockIn")
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
    let _ = eframe::run_native("LockIn", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, s)))));
}

/// One overlay at a time (hotkeys and the camera are exclusive anyway).
fn single_instance() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    // SAFETY: named mutex lives for the process lifetime (handle intentionally leaked).
    unsafe { CreateMutexW(None, true, w!("Local\\LockIn.SingleInstance")).is_ok() && GetLastError() != ERROR_ALREADY_EXISTS }
}

/// `lockin --diag [image ...]`: run the models on images (or 10 s of live camera when none given)
/// and print detections, poses and timings.
fn diag(images: &[String]) {
    use std::time::Instant;
    let t = Instant::now();
    let face = vision::FaceModel::load().expect("face model");
    println!("face model load {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let t = Instant::now();
    let obj = vision::ObjectModel::load().expect("object model");
    println!("object model load {:.0} ms", t.elapsed().as_secs_f64() * 1e3);
    let report = |f: &camera::Frame| {
        let t = Instant::now();
        let d = face.detect(f).unwrap();
        let l1 = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        let o = obj.detect(f).unwrap();
        let l2 = t.elapsed().as_secs_f64() * 1e3;
        match d {
            Some(d) => println!(
                "  face {:.2} rect {:?} pose {:?} | person {:.2} phone {:.2} | L1 {l1:.1} ms L2 {l2:.1} ms",
                d.score,
                d.rect.map(|v| (v * 100.0).round() / 100.0),
                d.pose(),
                o.person,
                o.phone
            ),
            None => println!("  no face | person {:.2} phone {:.2} | L1 {l1:.1} ms L2 {l2:.1} ms", o.person, o.phone),
        }
    };
    if images.is_empty() {
        let _ = camera::startup();
        println!("cameras: {:?}", camera::list());
        let mut cam = camera::Camera::open(0).expect("open camera");
        let mut f = camera::Frame { width: 0, height: 0, bgra: Vec::new() };
        for _ in 0..10 {
            let t = Instant::now();
            cam.grab(&mut f).expect("grab");
            let luma = f.bgra.chunks_exact(4).map(|p| p[1] as u64).sum::<u64>() / (f.width * f.height).max(1) as u64;
            println!("frame {}x{} grab {:.0} ms, mean brightness {luma}/255", f.width, f.height, t.elapsed().as_secs_f64() * 1e3);
            report(&f);
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    for path in images {
        let img = image::open(path).expect("open image").to_rgba8();
        let (w, h) = img.dimensions();
        let mut bgra = img.into_raw();
        for p in bgra.chunks_exact_mut(4) {
            p.swap(0, 2);
        }
        println!("{path} {w}x{h}");
        report(&camera::Frame { width: w as usize, height: h as usize, bgra });
    }
}
