//! Camera layers L1 (YuNet face + 5 landmarks -> head pose) and L2 (NanoDet person/phone), run on a
//! background thread with an adaptive frame rate. Models are embedded; L2 loads on demand and is
//! dropped again after a minute idle to give the memory back.

use crate::camera::{self, Camera, Frame};
use crate::config::Pose;
use crate::policy::CamObs;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};
use tract_onnx::prelude::*;

static YUNET: &[u8] = include_bytes!("../models/yunet.onnx");
static NANODET: &[u8] = include_bytes!("../models/nanodet.onnx");

const FACE_W: usize = 256;
const FACE_H: usize = 192;
const DET: usize = 416;
const COCO_PERSON: usize = 0;
const COCO_PHONE: usize = 67;

type Plan = Arc<TypedRunnableModel>;

fn load(bytes: &[u8], shape: [usize; 4]) -> TractResult<Plan> {
    tract_onnx::onnx()
        .model_for_read(&mut &bytes[..])?
        .with_input_fact(0, f32::fact(shape).into())?
        .into_optimized()?
        .into_runnable()
}

fn floats(t: &TValue) -> TractResult<&[f32]> {
    t.try_as_plain_ram()?.as_slice::<f32>()
}

#[derive(Clone, Copy, Debug)]
pub struct Face {
    pub score: f32,
    /// Box in normalised [0,1] frame coordinates: x, y, w, h.
    pub rect: [f32; 4],
    /// right eye, left eye, nose, right mouth, left mouth, in model-input pixels (isotropic, for pose ratios).
    pub landmarks: [[f32; 2]; 5],
}

impl Face {
    pub fn pose(&self) -> Pose {
        let [re, le, nose, rm, lm] = self.landmarks;
        let eye = [(re[0] + le[0]) / 2.0, (re[1] + le[1]) / 2.0];
        let mouth_y = (rm[1] + lm[1]) / 2.0;
        let eye_dist = ((le[0] - re[0]).powi(2) + (le[1] - re[1]).powi(2)).sqrt().max(1e-4);
        Pose {
            yaw: (nose[0] - eye[0]) / eye_dist,
            pitch: (nose[1] - eye[1]) / (mouth_y - eye[1]).max(1e-4),
            area: self.rect[2] * self.rect[3],
        }
    }
}

pub struct FaceModel(Plan);

impl FaceModel {
    pub fn load() -> TractResult<Self> {
        Ok(FaceModel(load(YUNET, [1, 3, FACE_H, FACE_W])?))
    }

    pub fn detect(&self, f: &Frame) -> TractResult<Option<Face>> {
        // Letterbox into 256x192 (keeps faces undistorted for 16:9 and 4:3 cameras), BGR planes, raw 0..255.
        let scale = (FACE_W as f32 / f.width as f32).min(FACE_H as f32 / f.height as f32);
        let (nw, nh) = ((f.width as f32 * scale) as usize, (f.height as f32 * scale) as usize);
        let mut data = vec![0f32; 3 * FACE_H * FACE_W];
        let plane = FACE_H * FACE_W;
        for y in 0..nh {
            let sy = (y * f.height / nh).min(f.height - 1);
            for x in 0..nw {
                let sx = (x * f.width / nw).min(f.width - 1);
                let p = &f.bgra[(sy * f.width + sx) * 4..];
                let i = y * FACE_W + x;
                data[i] = p[0] as f32;
                data[plane + i] = p[1] as f32;
                data[2 * plane + i] = p[2] as f32;
            }
        }
        let input = Tensor::from_shape(&[1, 3, FACE_H, FACE_W], &data)?;
        let out = self.0.run(tvec!(input.into()))?;
        let (cw, ch) = (nw as f32, nh as f32);
        let mut best: Option<Face> = None;
        for (k, stride) in [8usize, 16, 32].into_iter().enumerate() {
            let cls = floats(&out[k])?;
            let obj = floats(&out[3 + k])?;
            let bbox = floats(&out[6 + k])?;
            let kps = floats(&out[9 + k])?;
            let cols = FACE_W / stride;
            for idx in 0..cls.len() {
                let score = (cls[idx].clamp(0.0, 1.0) * obj[idx].clamp(0.0, 1.0)).sqrt();
                if score < 0.6 || best.is_some_and(|b| b.score >= score) {
                    continue;
                }
                let (r, c) = ((idx / cols) as f32, (idx % cols) as f32);
                let s = stride as f32;
                let b = &bbox[idx * 4..idx * 4 + 4];
                let (cx, cy) = ((c + b[0]) * s, (r + b[1]) * s);
                let (w, h) = (b[2].exp() * s, b[3].exp() * s);
                let mut landmarks = [[0f32; 2]; 5];
                for (n, l) in landmarks.iter_mut().enumerate() {
                    *l = [(kps[idx * 10 + 2 * n] + c) * s, (kps[idx * 10 + 2 * n + 1] + r) * s];
                }
                best = Some(Face { score, rect: [(cx - w / 2.0) / cw, (cy - h / 2.0) / ch, w / cw, h / ch], landmarks });
            }
        }
        Ok(best)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Objects {
    pub person: f32,
    pub phone: f32,
}

pub struct ObjectModel(Plan);

impl ObjectModel {
    pub fn load() -> TractResult<Self> {
        Ok(ObjectModel(load(NANODET, [1, 3, DET, DET])?))
    }

    /// Max class confidence for person and cell phone (letterboxed 416x416, BGR mean/std).
    pub fn detect(&self, f: &Frame) -> TractResult<Objects> {
        const MEAN: [f32; 3] = [103.53, 116.28, 123.675];
        const STD: [f32; 3] = [57.375, 57.12, 58.395];
        let scale = (DET as f32 / f.width as f32).min(DET as f32 / f.height as f32);
        let (nw, nh) = ((f.width as f32 * scale) as usize, (f.height as f32 * scale) as usize);
        let plane = DET * DET;
        let mut data = vec![0f32; 3 * plane];
        for c in 0..3 {
            data[c * plane..(c + 1) * plane].fill((0.0 - MEAN[c]) / STD[c]);
        }
        for y in 0..nh {
            let sy = (y * f.height / nh).min(f.height - 1);
            for x in 0..nw {
                let sx = (x * f.width / nw).min(f.width - 1);
                let p = &f.bgra[(sy * f.width + sx) * 4..];
                let i = y * DET + x;
                for c in 0..3 {
                    data[c * plane + i] = (p[c] as f32 - MEAN[c]) / STD[c];
                }
            }
        }
        let input = Tensor::from_shape(&[1, 3, DET, DET], &data)?;
        let out = self.0.run(tvec!(input.into()))?;
        let mut o = Objects::default();
        for t in out.iter().take(3) {
            for row in floats(t)?.chunks_exact(80) {
                o.person = o.person.max(row[COCO_PERSON]);
                o.phone = o.phone.max(row[COCO_PHONE]);
            }
        }
        Ok(o)
    }
}

pub enum Cmd {
    Start { camera: u32, interval: f64 },
    Stop,
    Fast(bool),
    RunL2,
    /// Average a few frames into a pose for calibration.
    Calibrate { camera: u32, target: CalTarget },
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalTarget {
    Screen,
    Book,
}

pub enum Msg {
    Obs { obs: CamObs, face: Option<Face>, l1_ms: f32, l2: Option<(Objects, f32)> },
    Calibrated(CalTarget, Option<Pose>),
    Error(String),
}

// NanoDet-Plus scores run low: clear people ~0.5-0.8. Clear phones scored ~0.7, while 320px live
// webcam frames without a phone reached ~0.44, so the phone bar sits at 0.5 (+2 hits in policy).
const PERSON_MIN: f32 = 0.4;
const PHONE_MIN: f32 = 0.5;
const FAST_SECS: f64 = 0.7;
const L2_IDLE_DROP: Duration = Duration::from_secs(60);

struct Worker {
    tx: Sender<Msg>,
    wake: Box<dyn Fn() + Send>,
    cam: Option<(u32, Camera)>,
    face: Option<FaceModel>,
    obj: Option<(ObjectModel, Instant)>,
    frame: Frame,
}

impl Worker {
    fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
        (self.wake)();
    }

    fn ensure(&mut self, index: u32) -> Result<(), String> {
        if self.cam.as_ref().map(|(i, _)| *i) != Some(index) {
            self.cam = None;
            let cam = Camera::open(index).map_err(|e| format!("Camera unavailable: {}", e.message()))?;
            self.cam = Some((index, cam));
        }
        if self.face.is_none() {
            self.face = Some(FaceModel::load().map_err(|e| format!("Face model failed: {e}"))?);
        }
        Ok(())
    }

    fn sample(&mut self, l2: bool) -> Result<(), String> {
        let (_, cam) = self.cam.as_mut().ok_or("camera closed")?;
        cam.grab(&mut self.frame).map_err(|e| format!("Camera read failed: {}", e.message()))?;
        let t = Instant::now();
        let face = self.face.as_ref().unwrap().detect(&self.frame).map_err(|e| e.to_string())?;
        let l1_ms = t.elapsed().as_secs_f32() * 1000.0;
        let mut obs = CamObs { face: face.map(|f| f.pose()), ..Default::default() };
        let mut l2_out = None;
        if l2 {
            if self.obj.is_none() {
                self.obj = Some((ObjectModel::load().map_err(|e| format!("Object model failed: {e}"))?, Instant::now()));
            }
            let (model, used) = self.obj.as_mut().unwrap();
            *used = Instant::now();
            let t = Instant::now();
            let o = model.detect(&self.frame).map_err(|e| e.to_string())?;
            obs.person = Some(o.person >= PERSON_MIN || face.is_some());
            obs.phone = Some(o.phone >= PHONE_MIN);
            l2_out = Some((o, t.elapsed().as_secs_f32() * 1000.0));
        }
        self.send(Msg::Obs { obs, face, l1_ms, l2: l2_out });
        Ok(())
    }

    fn calibrate(&mut self, index: u32, target: CalTarget) -> Result<Option<Pose>, String> {
        self.ensure(index)?;
        let mut poses = Vec::new();
        for _ in 0..8 {
            let (_, cam) = self.cam.as_mut().unwrap();
            cam.grab(&mut self.frame).map_err(|e| e.message().to_string())?;
            if let Some(f) = self.face.as_ref().unwrap().detect(&self.frame).map_err(|e| e.to_string())? {
                poses.push(f.pose());
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let _ = target;
        if poses.len() < 3 {
            return Ok(None);
        }
        let n = poses.len() as f32;
        let mut avg = Pose::default();
        for p in &poses {
            avg.yaw += p.yaw / n;
            avg.pitch += p.pitch / n;
            avg.area += p.area / n;
        }
        Ok(Some(avg))
    }
}

pub fn spawn(rx: Receiver<Cmd>, tx: Sender<Msg>, wake: Box<dyn Fn() + Send>) {
    std::thread::Builder::new()
        .name("vision".into())
        .stack_size(4 << 20)
        .spawn(move || {
            // SAFETY: per-thread COM/MF init for the capture objects created here.
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
            }
            let _ = camera::startup();
            let mut w = Worker { tx, wake, cam: None, face: None, obj: None, frame: Frame { width: 0, height: 0, bgra: Vec::new() } };
            let mut running: Option<(u32, f64)> = None;
            let mut fast = false;
            let mut want_l2 = false;
            let mut next = Instant::now();
            loop {
                let cmd = if running.is_some() {
                    match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
                        Ok(c) => Some(c),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                } else {
                    match rx.recv() {
                        Ok(c) => Some(c),
                        Err(_) => return,
                    }
                };
                match cmd {
                    Some(Cmd::Start { camera, interval }) => {
                        running = Some((camera, interval));
                        next = Instant::now();
                        continue;
                    }
                    Some(Cmd::Stop) => {
                        running = None;
                        fast = false;
                        want_l2 = false;
                        // Release the camera (LED off) and the models between focus blocks.
                        w.cam = None;
                        w.face = None;
                        w.obj = None;
                        continue;
                    }
                    Some(Cmd::Fast(f)) => {
                        fast = f;
                        if f {
                            next = next.min(Instant::now() + Duration::from_secs_f64(FAST_SECS));
                        }
                        continue;
                    }
                    Some(Cmd::RunL2) => {
                        want_l2 = true;
                        next = Instant::now();
                        continue;
                    }
                    Some(Cmd::Calibrate { camera, target }) => {
                        let r = w.calibrate(camera, target);
                        match r {
                            Ok(p) => w.send(Msg::Calibrated(target, p)),
                            Err(e) => w.send(Msg::Error(e)),
                        }
                        if running.is_none() {
                            w.cam = None;
                            w.face = None;
                        }
                        continue;
                    }
                    Some(Cmd::Quit) => return,
                    None => {}
                }
                let Some((index, interval)) = running else { continue };
                let res = w.ensure(index).and_then(|_| w.sample(want_l2));
                want_l2 = false;
                if let Err(e) = res {
                    w.cam = None;
                    w.send(Msg::Error(e));
                    next = Instant::now() + Duration::from_secs(10);
                    continue;
                }
                if w.obj.as_ref().is_some_and(|(_, used)| used.elapsed() > L2_IDLE_DROP) {
                    w.obj = None;
                }
                next = Instant::now() + Duration::from_secs_f64(if fast { FAST_SECS } else { interval });
            }
        })
        .expect("spawn vision thread");
}
