//! Camera L1 (YuNet face box + MediaPipe Face Landmarker mesh and blendshapes), L2 (RTMDet-tiny
//! person/phone), guided calibration and the dashboard preview feed. Models and camera frames stay
//! in memory.

use crate::camera::{self, Camera, Frame};
use crate::config::{Calibration, FeatureSample, Pose};
use crate::policy::{classify_gaze, classify_gaze_features, CamObs, Gaze};
use image::codecs::jpeg::JpegEncoder;
use image::ExtendedColorType;
use serde::Serialize;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tract_onnx::prelude::*;

static YUNET: &[u8] = include_bytes!("../models/yunet.onnx");
static MESH_MODEL: &[u8] = include_bytes!("../models/face_landmarks.onnx");
static BLENDSHAPES: &[u8] = include_bytes!("../models/face_blendshapes.onnx");
static RTMDET: &[u8] = include_bytes!("../models/rtmdet.onnx");

const FACE_W: usize = 256;
const FACE_H: usize = 192;
const MESH: usize = 256;
const MESH_POINTS: usize = 478;
const DET: usize = 416;
const COCO_PERSON: usize = 0;
const COCO_PHONE: usize = 67;

type Plan = Arc<TypedRunnableModel>;

fn load(bytes: &[u8], shape: &[usize]) -> TractResult<Plan> {
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
    /// Right eye, left eye, nose, right mouth, left mouth in normalised frame coordinates.
    pub landmarks: [[f32; 2]; 5],
    aspect: f32,
}

impl Face {
    fn metric_points(&self) -> [[f32; 2]; 5] {
        self.landmarks.map(|p| [p[0] * self.aspect, p[1]])
    }

    /// Legacy pose ratios are retained so old settings and the uncalibrated fallback behave as before.
    pub fn pose(&self) -> Pose {
        let [re, le, nose, rm, lm] = self.metric_points();
        let eye = [(re[0] + le[0]) / 2.0, (re[1] + le[1]) / 2.0];
        let roll = (le[1] - re[1]).atan2(le[0] - re[0]);
        let rotate = |p: [f32; 2]| {
            let (s, c) = (-roll).sin_cos();
            let (x, y) = (p[0] - eye[0], p[1] - eye[1]);
            [x * c - y * s + eye[0], x * s + y * c + eye[1]]
        };
        let (re, le, nose, rm, lm) = (rotate(re), rotate(le), rotate(nose), rotate(rm), rotate(lm));
        let eye = [(re[0] + le[0]) / 2.0, (re[1] + le[1]) / 2.0];
        let mouth_y = (rm[1] + lm[1]) / 2.0;
        let eye_dist = (le[0] - re[0]).abs().max(1e-4);
        Pose {
            yaw: (nose[0] - eye[0]) / eye_dist,
            pitch: (nose[1] - eye[1]) / (mouth_y - eye[1]).max(1e-4),
            area: self.rect[2] * self.rect[3],
            roll: roll.to_degrees(),
            center: [
                self.rect[0] + self.rect[2] * 0.5,
                self.rect[1] + self.rect[3] * 0.5,
            ],
        }
    }
}

pub struct FaceModel(Plan);

impl FaceModel {
    pub fn load() -> TractResult<Self> {
        Ok(Self(load(YUNET, &[1, 3, FACE_H, FACE_W])?))
    }

    pub fn detect(&self, f: &Frame) -> TractResult<Option<Face>> {
        let scale = (FACE_W as f32 / f.width as f32).min(FACE_H as f32 / f.height as f32);
        let (nw, nh) = (
            (f.width as f32 * scale) as usize,
            (f.height as f32 * scale) as usize,
        );
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
                    *l = [
                        (kps[idx * 10 + 2 * n] + c) * s / cw,
                        (kps[idx * 10 + 2 * n + 1] + r) * s / ch,
                    ];
                }
                best = Some(Face {
                    score,
                    rect: [(cx - w / 2.0) / cw, (cy - h / 2.0) / ch, w / cw, h / ch],
                    landmarks,
                    aspect: f.width as f32 / f.height as f32,
                });
            }
        }
        Ok(best)
    }
}

#[derive(Clone, Serialize)]
pub struct EyeOverlay {
    pub contour: Vec<[f32; 2]>,
    pub iris: [f32; 2],
    pub iris_r: f32,
    pub open: f32,
}

#[derive(Clone, Default, Serialize)]
pub struct Overlay {
    pub face: Option<[f32; 4]>,
    pub landmarks: Vec<[f32; 2]>,
    pub eyes: Vec<EyeOverlay>,
    pub head: Option<[f32; 3]>,
    pub gaze: Option<[f32; 2]>,
    pub label: Option<String>,
    pub confidence: f32,
    pub person: Option<f32>,
    pub phone: Option<f32>,
    pub l1_ms: f32,
}

pub struct PreviewFrame {
    pub seq: u64,
    pub jpeg: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub overlay: Overlay,
}

pub type PreviewSlot = Arc<Mutex<Option<PreviewFrame>>>;

#[derive(Clone)]
struct EyeMeasure {
    overlay: EyeOverlay,
    iris_x: f32,
    iris_y: f32,
}

/// Face Landmarker output for one face.
#[derive(Clone)]
pub(crate) struct Mesh {
    /// All 478 points in normalised frame coordinates (for the overlay).
    points: Vec<[f32; 2]>,
    /// Image-left eye first (the subject's right eye), then image-right.
    eyes: [EyeMeasure; 2],
    /// Yaw, pitch (positive = looking down), roll in degrees.
    head: [f32; 3],
    /// Eye-look blendshapes averaged over both eyes: horizontal (out minus in, signed so both eyes
    /// agree) and vertical (up minus down).
    look: [f32; 2],
}

impl Mesh {
    pub(crate) fn overlays(&self) -> Vec<EyeOverlay> {
        self.eyes.iter().map(|e| e.overlay.clone()).collect()
    }

    pub(crate) fn head(&self) -> [f32; 3] {
        self.head
    }

    pub(crate) fn direction(&self) -> [f32; 2] {
        let x = (self.eyes[0].iris_x + self.eyes[1].iris_x) * 0.5;
        let y = (self.eyes[0].iris_y + self.eyes[1].iris_y) * 0.5;
        [
            ((x - 0.5) * 2.0).clamp(-1.0, 1.0),
            ((y - 0.5) * 2.0).clamp(-1.0, 1.0),
        ]
    }
}

/// Mesh indices for one eye: 16-point lid contour (outer corner, lower lid, inner corner, upper
/// lid), iris centre (the next four indices are the iris rim), upper and lower lid midpoints.
struct EyeIdx {
    contour: [usize; 16],
    iris: usize,
    upper: usize,
    lower: usize,
}

const EYES: [EyeIdx; 2] = [
    EyeIdx {
        contour: [33, 7, 163, 144, 145, 153, 154, 155, 133, 173, 157, 158, 159, 160, 161, 246],
        iris: 468,
        upper: 159,
        lower: 145,
    },
    EyeIdx {
        contour: [263, 249, 390, 373, 374, 380, 381, 382, 362, 398, 384, 385, 386, 387, 388, 466],
        iris: 473,
        upper: 386,
        lower: 374,
    },
];

/// Landmarks the blendshape model takes, in this order (MediaPipe `face_blendshapes_graph.cc`).
const BLENDSHAPE_INPUT: [usize; 146] = [
    0, 1, 4, 5, 6, 7, 8, 10, 13, 14, 17, 21, 33, 37, 39, 40, 46, 52, 53, 54, 55, 58, 61, 63, 65, 66, 67, 70, 78, 80,
    81, 82, 84, 87, 88, 91, 93, 95, 103, 105, 107, 109, 127, 132, 133, 136, 144, 145, 146, 148, 149, 150, 152, 153,
    154, 155, 157, 158, 159, 160, 161, 162, 163, 168, 172, 173, 176, 178, 181, 185, 191, 195, 197, 234, 246, 249, 251,
    263, 267, 269, 270, 276, 282, 283, 284, 285, 288, 291, 293, 295, 296, 297, 300, 308, 310, 311, 312, 314, 317, 318,
    321, 323, 324, 332, 334, 336, 338, 356, 361, 362, 365, 373, 374, 375, 377, 378, 379, 380, 381, 382, 384, 385, 386,
    387, 388, 389, 390, 397, 398, 400, 402, 405, 409, 415, 454, 466, 468, 469, 470, 471, 472, 473, 474, 475, 476, 477,
];
/// Blendshape output indices: eyeLook{Down,In,Out,Up}{Left,Right}.
const LOOK_DOWN: [usize; 2] = [11, 12];
const LOOK_IN: [usize; 2] = [13, 14];
const LOOK_OUT: [usize; 2] = [15, 16];
const LOOK_UP: [usize; 2] = [17, 18];

fn sample_bilinear(f: &Frame, x: f32, y: f32) -> [f32; 3] {
    let x = x.clamp(0.0, (f.width - 1) as f32);
    let y = y.clamp(0.0, (f.height - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(f.width - 1), (y0 + 1).min(f.height - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let pixel = |px: usize, py: usize| &f.bgra[(py * f.width + px) * 4..][..3];
    let (p00, p10, p01, p11) = (pixel(x0, y0), pixel(x1, y0), pixel(x0, y1), pixel(x1, y1));
    let channel = |bgr: usize| {
        let top = p00[bgr] as f32 * (1.0 - fx) + p10[bgr] as f32 * fx;
        let bottom = p01[bgr] as f32 * (1.0 - fx) + p11[bgr] as f32 * fx;
        (top * (1.0 - fy) + bottom * fy) / 255.0
    };
    [channel(2), channel(1), channel(0)]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn unit(v: [f32; 3]) -> [f32; 3] {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    [v[0] / n, v[1] / n, v[2] / n]
}

/// Head angles from the mesh: the eye-corner line is the head's x axis, forehead-to-chin its y
/// axis. `p` is in crop pixels, where z shares x's scale; `crop_roll` is the crop's rotation.
fn head_angles(p: impl Fn(usize) -> [f32; 3], crop_roll: f32) -> [f32; 3] {
    let x = unit(sub(p(263), p(33)));
    let down = sub(p(152), p(10));
    let d = down[0] * x[0] + down[1] * x[1] + down[2] * x[2];
    let y = unit([down[0] - d * x[0], down[1] - d * x[1], down[2] - d * x[2]]);
    [
        x[2].atan2(x[0]).to_degrees(),
        y[2].atan2(y[1]).to_degrees(),
        (crop_roll + x[1].atan2(x[0])).to_degrees(),
    ]
}

fn eye_measure(
    idx: &EyeIdx,
    p: &impl Fn(usize) -> [f32; 3],
    to_frame: &impl Fn(f32, f32) -> [f32; 2],
    crop_px_to_frame_w: f32,
) -> Option<EyeMeasure> {
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for &i in &idx.contour {
        let q = p(i);
        for d in 0..2 {
            min[d] = min[d].min(q[d]);
            max[d] = max[d].max(q[d]);
        }
    }
    let width = (max[0] - min[0]).max(1e-3);
    let iris = p(idx.iris);
    let (upper, lower) = (p(idx.upper)[1], p(idx.lower)[1]);
    let iris_x = ((iris[0] - min[0]) / width).clamp(-0.25, 1.25);
    // Normalising by the instantaneous lid gap makes blinks look like huge gaze shifts.
    // Measure vertical iris displacement from the lid centre in eye-width units instead;
    // the factor maps the normal visible eye range approximately onto 0..1.
    let iris_y = (0.5 + (iris[1] - (upper + lower) * 0.5) / width * 3.0).clamp(-0.25, 1.25);
    let open = ((lower - upper).abs() / width).clamp(0.0, 2.0);
    if !iris_x.is_finite() || !iris_y.is_finite() || !open.is_finite() {
        return None;
    }
    let rim: f32 = (1..5)
        .map(|k| {
            let r = p(idx.iris + k);
            (r[0] - iris[0]).hypot(r[1] - iris[1])
        })
        .sum();
    Some(EyeMeasure {
        overlay: EyeOverlay {
            contour: idx.contour.iter().map(|&i| to_frame(p(i)[0], p(i)[1])).collect(),
            iris: to_frame(iris[0], iris[1]),
            iris_r: rim / 4.0 * crop_px_to_frame_w,
            open,
        },
        iris_x,
        iris_y,
    })
}

/// MediaPipe Face Landmarker: 478-point mesh (with irises) on a face crop, then 52 blendshapes
/// from a subset of those points.
pub struct MeshModel {
    mesh: Plan,
    blendshapes: Plan,
}

impl MeshModel {
    pub fn load() -> TractResult<Self> {
        Ok(Self {
            mesh: load(MESH_MODEL, &[1, MESH, MESH, 3])?,
            blendshapes: load(BLENDSHAPES, &[1, BLENDSHAPE_INPUT.len(), 2])?,
        })
    }

    pub(crate) fn detect(&self, f: &Frame, face: &Face) -> TractResult<Option<Mesh>> {
        let (fw, fh) = (f.width as f32, f.height as f32);
        let (re, le) = (face.landmarks[0], face.landmarks[1]);
        let roll = ((le[1] - re[1]) * fh).atan2((le[0] - re[0]) * fw);
        let center = [(face.rect[0] + face.rect[2] * 0.5) * fw, (face.rect[1] + face.rect[3] * 0.5) * fh];
        // MediaPipe's crop: the detection box squared on its long side, enlarged 1.5x, rotated level.
        let side = (face.rect[2] * fw).max(face.rect[3] * fh) * 1.5;
        if !(side >= 16.0) || !roll.is_finite() {
            return Ok(None);
        }
        let (sn, cs) = roll.sin_cos();
        let to_px = |x: f32, y: f32| {
            let (lx, ly) = ((x / MESH as f32 - 0.5) * side, (y / MESH as f32 - 0.5) * side);
            [center[0] + lx * cs - ly * sn, center[1] + lx * sn + ly * cs]
        };
        // NHWC, RGB in 0..1.
        let mut data = vec![0f32; MESH * MESH * 3];
        for y in 0..MESH {
            for x in 0..MESH {
                let [sx, sy] = to_px(x as f32 + 0.5, y as f32 + 0.5);
                data[(y * MESH + x) * 3..][..3].copy_from_slice(&sample_bilinear(f, sx, sy));
            }
        }
        let out = self.mesh.run(tvec!(Tensor::from_shape(&[1, MESH, MESH, 3], &data)?.into()))?;
        let pts = floats(&out[0])?;
        let presence = floats(&out[1])?.first().copied().unwrap_or(f32::NEG_INFINITY);
        if pts.len() < MESH_POINTS * 3 {
            return Err(std::io::Error::other("unexpected face mesh output").into());
        }
        // Presence is a logit; below 0 the crop holds no face.
        if !(presence > 0.0) {
            return Ok(None);
        }
        let p = |i: usize| [pts[i * 3], pts[i * 3 + 1], pts[i * 3 + 2]];
        let to_frame = |x: f32, y: f32| {
            let [px, py] = to_px(x, y);
            [px / fw, py / fh]
        };
        let crop_px_to_frame_w = side / MESH as f32 / fw;
        let (Some(right), Some(left)) = (
            eye_measure(&EYES[0], &p, &to_frame, crop_px_to_frame_w),
            eye_measure(&EYES[1], &p, &to_frame, crop_px_to_frame_w),
        ) else {
            return Ok(None);
        };

        // The blendshape model takes the subset in frame pixels.
        let mut input = Vec::with_capacity(BLENDSHAPE_INPUT.len() * 2);
        for &i in &BLENDSHAPE_INPUT {
            input.extend(to_px(pts[i * 3], pts[i * 3 + 1]));
        }
        let out = self
            .blendshapes
            .run(tvec!(Tensor::from_shape(&[1, BLENDSHAPE_INPUT.len(), 2], &input)?.into()))?;
        let b = floats(&out[0])?;
        if b.len() < 52 {
            return Err(std::io::Error::other("unexpected blendshape output").into());
        }
        let look = [
            ((b[LOOK_OUT[0]] - b[LOOK_IN[0]]) - (b[LOOK_OUT[1]] - b[LOOK_IN[1]])) * 0.5,
            ((b[LOOK_UP[0]] + b[LOOK_UP[1]]) - (b[LOOK_DOWN[0]] + b[LOOK_DOWN[1]])) * 0.5,
        ];
        Ok(Some(Mesh {
            points: (0..MESH_POINTS).map(|i| to_frame(pts[i * 3], pts[i * 3 + 1])).collect(),
            eyes: [right, left],
            head: head_angles(p, roll),
            look,
        }))
    }
}

pub(crate) fn feature_sample(face: &Face, mesh: &Mesh) -> FeatureSample {
    let h = mesh.head;
    let p = face.pose();
    let [r, l] = &mesh.eyes;
    FeatureSample {
        values: vec![
            h[0],
            h[1],
            h[2],
            r.iris_x,
            r.iris_y,
            l.iris_x,
            l.iris_y,
            r.overlay.open,
            l.overlay.open,
            p.area,
            p.center[0],
            p.center[1],
            mesh.look[0],
            mesh.look[1],
        ],
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Objects {
    pub person: f32,
    pub phone: f32,
}

/// RTMDet-tiny (COCO), cut before NMS: per-anchor sigmoid scores for the 80 classes. Only the
/// best person and phone scores are used, so boxes are not decoded.
pub struct ObjectModel(Plan);

impl ObjectModel {
    pub fn load() -> TractResult<Self> {
        Ok(Self(load(RTMDET, &[1, 3, DET, DET])?))
    }

    pub fn detect(&self, f: &Frame) -> TractResult<Objects> {
        // MMDetection preprocessing: BGR, letterboxed top-left onto grey 114, mean/std normalised.
        const MEAN: [f32; 3] = [103.53, 116.28, 123.675];
        const STD: [f32; 3] = [57.375, 57.12, 58.395];
        const PAD: f32 = 114.0;
        let scale = (DET as f32 / f.width as f32).min(DET as f32 / f.height as f32);
        let (nw, nh) = (
            (f.width as f32 * scale) as usize,
            (f.height as f32 * scale) as usize,
        );
        let plane = DET * DET;
        let mut data = vec![0f32; 3 * plane];
        for c in 0..3 {
            data[c * plane..(c + 1) * plane].fill((PAD - MEAN[c]) / STD[c]);
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
        for row in floats(&out[0])?.chunks_exact(80) {
            o.person = o.person.max(row[COCO_PERSON]);
            o.phone = o.phone.max(row[COCO_PHONE]);
        }
        Ok(o)
    }
}

pub enum Cmd {
    Start { camera: u32, interval: f64 },
    Stop,
    Fast(bool),
    RunL2,
    /// Live preview for the dashboard: `Some(camera)` on, `None` off.
    Preview(Option<u32>),
    SetCalibration(Calibration),
    Calibrate { camera: u32, target: CalTarget },
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CalTarget {
    Screen,
    Book,
    Away,
}

pub enum Msg {
    Obs {
        obs: CamObs,
        face: Option<Face>,
        eyes: Option<Vec<EyeOverlay>>,
        gaze: Option<(Gaze, f32)>,
        l1_ms: f32,
        l2: Option<(Objects, f32)>,
    },
    CalProgress {
        target: CalTarget,
        progress: f32,
        samples: usize,
    },
    Calibrated {
        target: CalTarget,
        samples: Vec<FeatureSample>,
        error: Option<String>,
    },
    Error(String),
}

const PERSON_MIN: f32 = 0.4;
/// RTMDet's quality-focal scores run lower than NanoDet's: hand-held phones scored 0.50-0.56 in
/// test photos, a phone-free portrait 0.12. The policy still needs two hits in a row.
const PHONE_MIN: f32 = 0.4;
const FAST_SECS: f64 = 0.7;
const PREVIEW_SECS: f64 = 1.0 / 12.0;
const L2_IDLE_DROP: Duration = Duration::from_secs(60);

struct Analysis {
    face: Option<Face>,
    mesh: Option<Mesh>,
    gaze: Option<(Gaze, f32)>,
    feature: Option<FeatureSample>,
    obs: CamObs,
    l1_ms: f32,
    l2: Option<(Objects, f32)>,
}

struct Worker {
    tx: Sender<Msg>,
    wake: Box<dyn Fn() + Send>,
    preview: PreviewSlot,
    seq: u64,
    cam: Option<(u32, Camera)>,
    face: Option<FaceModel>,
    mesh: Option<MeshModel>,
    obj: Option<(ObjectModel, Instant)>,
    last_objects: Option<Objects>,
    calibration: Calibration,
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
            let cam =
                Camera::open(index).map_err(|e| format!("Camera unavailable: {}", e.message()))?;
            self.cam = Some((index, cam));
        }
        if self.face.is_none() {
            self.face = Some(FaceModel::load().map_err(|e| format!("Face model failed: {e}"))?);
        }
        if self.mesh.is_none() {
            self.mesh = Some(MeshModel::load().map_err(|e| format!("Face mesh model failed: {e}"))?);
        }
        Ok(())
    }

    fn capture(&mut self, run_l2: bool) -> Result<Analysis, String> {
        let (_, cam) = self.cam.as_mut().ok_or("camera closed")?;
        cam.grab(&mut self.frame)
            .map_err(|e| format!("Camera read failed: {}", e.message()))?;
        let t = Instant::now();
        let face = self
            .face
            .as_ref()
            .unwrap()
            .detect(&self.frame)
            .map_err(|e| e.to_string())?;
        let mesh = match &face {
            Some(f) => self
                .mesh
                .as_ref()
                .unwrap()
                .detect(&self.frame, f)
                .map_err(|e| e.to_string())?,
            None => None,
        };
        let l1_ms = t.elapsed().as_secs_f32() * 1000.0;
        let feature = face
            .as_ref()
            .zip(mesh.as_ref())
            .map(|(f, m)| feature_sample(f, m));
        let gaze = match (face, feature.as_ref()) {
            (Some(f), Some(s)) => classify_gaze_features(s, &self.calibration)
                .or_else(|| Some((classify_gaze(f.pose(), &self.calibration), 0.45))),
            (Some(f), None) => Some((classify_gaze(f.pose(), &self.calibration), 0.35)),
            (None, _) => Some((Gaze::Absent, 1.0)),
        };
        let mut obs = CamObs {
            face: face.map(|f| f.pose()),
            gaze,
            ..Default::default()
        };
        let mut l2 = None;
        if run_l2 {
            if self.obj.is_none() {
                self.obj = Some((
                    ObjectModel::load().map_err(|e| format!("Object model failed: {e}"))?,
                    Instant::now(),
                ));
            }
            let (model, used) = self.obj.as_mut().unwrap();
            *used = Instant::now();
            let t = Instant::now();
            let o = model.detect(&self.frame).map_err(|e| e.to_string())?;
            obs.person = Some(o.person >= PERSON_MIN || face.is_some());
            obs.phone = Some(o.phone >= PHONE_MIN);
            self.last_objects = Some(o);
            l2 = Some((o, t.elapsed().as_secs_f32() * 1000.0));
        }
        Ok(Analysis {
            face,
            mesh,
            gaze,
            feature,
            obs,
            l1_ms,
            l2,
        })
    }

    fn emit(&self, a: &Analysis) {
        self.send(Msg::Obs {
            obs: a.obs,
            face: a.face,
            eyes: a.mesh.as_ref().map(Mesh::overlays),
            gaze: a.gaze,
            l1_ms: a.l1_ms,
            l2: a.l2,
        });
    }

    fn preview(&mut self, a: &Analysis) -> Result<(), String> {
        let (jpeg, width, height) = encode_preview(&self.frame)?;
        let label = a.gaze.map(|(g, _)| {
            match g {
                Gaze::Screen => "screen",
                Gaze::Book => "book",
                Gaze::Away => "away",
                Gaze::Absent => "absent",
            }
            .to_string()
        });
        let last = self.last_objects;
        let overlay = Overlay {
            face: a.face.map(|f| f.rect),
            landmarks: match (&a.mesh, a.face) {
                (Some(m), _) => m.points.clone(),
                (None, Some(f)) => f.landmarks.to_vec(),
                (None, None) => Vec::new(),
            },
            eyes: a.mesh.as_ref().map(Mesh::overlays).unwrap_or_default(),
            head: a.mesh.as_ref().map(Mesh::head),
            gaze: a.mesh.as_ref().map(Mesh::direction),
            label,
            confidence: a.gaze.map_or(0.0, |g| g.1),
            person: last.map(|o| o.person),
            phone: last.map(|o| o.phone),
            l1_ms: a.l1_ms,
        };
        self.seq = self.seq.wrapping_add(1);
        *self
            .preview
            .lock()
            .map_err(|_| "preview slot poisoned".to_string())? = Some(PreviewFrame {
            seq: self.seq,
            jpeg,
            width,
            height,
            overlay,
        });
        (self.wake)();
        Ok(())
    }

    fn calibrate(
        &mut self,
        index: u32,
        target: CalTarget,
        show_preview: bool,
    ) -> Result<Vec<FeatureSample>, String> {
        self.ensure(index)?;
        const COUNT: usize = 34;
        const STEP: Duration = Duration::from_millis(150);
        let started = Instant::now();
        let mut samples = Vec::with_capacity(COUNT);
        for i in 0..COUNT {
            let a = self.capture(false)?;
            if let Some(s) = a.feature.clone() {
                samples.push(s);
            }
            if show_preview {
                self.preview(&a)?;
            }
            if i % 2 == 0 || i + 1 == COUNT {
                self.send(Msg::CalProgress {
                    target,
                    progress: (i + 1) as f32 / COUNT as f32,
                    samples: samples.len(),
                });
            }
            std::thread::sleep(
                (started + STEP * (i as u32 + 1)).saturating_duration_since(Instant::now()),
            );
        }
        if samples.len() < 10 {
            Err(format!(
                "Only {} usable eye samples; keep both eyes visible and retry.",
                samples.len()
            ))
        } else {
            Ok(samples)
        }
    }

    fn store_calibration(&mut self, target: CalTarget, samples: Vec<FeatureSample>) {
        match target {
            CalTarget::Screen => self.calibration.screen_samples = samples,
            CalTarget::Book => self.calibration.book_samples = samples,
            CalTarget::Away => self.calibration.away_samples = samples,
        }
    }

    fn release_l1(&mut self) {
        self.cam = None;
        self.face = None;
        self.mesh = None;
        self.obj = None;
    }
}

fn encode_preview(f: &Frame) -> Result<(Vec<u8>, u32, u32), String> {
    let width = f.width.min(480);
    let height = (f.height * width / f.width).max(1);
    let mut rgb = vec![0u8; width * height * 3];
    for y in 0..height {
        let sy = (y * f.height / height).min(f.height - 1);
        for x in 0..width {
            let sx = (x * f.width / width).min(f.width - 1);
            let src = &f.bgra[(sy * f.width + sx) * 4..];
            let dst = &mut rgb[(y * width + x) * 3..];
            dst[..3].copy_from_slice(&[src[2], src[1], src[0]]);
        }
    }
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 70)
        .encode(&rgb, width as u32, height as u32, ExtendedColorType::Rgb8)
        .map_err(|e| format!("Preview JPEG failed: {e}"))?;
    Ok((jpeg, width as u32, height as u32))
}

pub fn spawn(rx: Receiver<Cmd>, tx: Sender<Msg>, wake: Box<dyn Fn() + Send>, preview: PreviewSlot) {
    std::thread::Builder::new()
        .name("vision".into())
        .stack_size(4 << 20)
        .spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_MULTITHREADED,
                );
            }
            let _ = camera::startup();
            let mut w = Worker {
                tx,
                wake,
                preview,
                seq: 0,
                cam: None,
                face: None,
                mesh: None,
                obj: None,
                last_objects: None,
                calibration: Calibration::default(),
                frame: Frame {
                    width: 0,
                    height: 0,
                    bgra: Vec::new(),
                },
            };
            let mut running: Option<(u32, f64)> = None;
            let mut previewing = false;
            let mut camera_index = 0;
            let mut fast = false;
            let mut want_l2 = false;
            let mut next_monitor = Instant::now();
            let mut next_preview = Instant::now();
            loop {
                let active = running.is_some() || previewing;
                let deadline = match (running.is_some(), previewing) {
                    (true, true) => next_monitor.min(next_preview),
                    (true, false) => next_monitor,
                    (false, true) => next_preview,
                    (false, false) => Instant::now(),
                };
                let cmd = if active {
                    match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
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
                        camera_index = camera;
                        running = Some((camera, interval));
                        next_monitor = Instant::now();
                        continue;
                    }
                    Some(Cmd::Stop) => {
                        running = None;
                        fast = false;
                        want_l2 = false;
                        if !previewing {
                            w.release_l1();
                        }
                        continue;
                    }
                    Some(Cmd::Preview(camera)) => {
                        previewing = camera.is_some();
                        if let Some(index) = camera {
                            camera_index = index;
                            next_preview = Instant::now();
                        } else {
                            if let Ok(mut slot) = w.preview.lock() {
                                *slot = None;
                            }
                            if running.is_none() {
                                w.release_l1();
                            }
                        }
                        continue;
                    }
                    Some(Cmd::SetCalibration(cal)) => {
                        w.calibration = cal;
                        continue;
                    }
                    Some(Cmd::Fast(on)) => {
                        fast = on;
                        if on {
                            next_monitor = next_monitor
                                .min(Instant::now() + Duration::from_secs_f64(FAST_SECS));
                        }
                        continue;
                    }
                    Some(Cmd::RunL2) => {
                        want_l2 = true;
                        next_monitor = Instant::now();
                        continue;
                    }
                    Some(Cmd::Calibrate { camera, target }) => {
                        camera_index = camera;
                        match w.calibrate(camera, target, previewing) {
                            Ok(samples) => {
                                w.store_calibration(target, samples.clone());
                                w.send(Msg::Calibrated {
                                    target,
                                    samples,
                                    error: None,
                                });
                            }
                            Err(error) => w.send(Msg::Calibrated {
                                target,
                                samples: Vec::new(),
                                error: Some(error),
                            }),
                        }
                        if running.is_none() && !previewing {
                            w.release_l1();
                        }
                        next_monitor = Instant::now();
                        next_preview = Instant::now();
                        continue;
                    }
                    Some(Cmd::Quit) => return,
                    None => {}
                }

                let now = Instant::now();
                let monitor_due = running.is_some() && now >= next_monitor;
                let preview_due = previewing && now >= next_preview;
                if !monitor_due && !preview_due {
                    continue;
                }
                let index = running.map(|r| r.0).unwrap_or(camera_index);
                let res = w
                    .ensure(index)
                    .and_then(|_| w.capture(monitor_due && want_l2));
                match res {
                    Ok(a) => {
                        if monitor_due {
                            w.emit(&a);
                            want_l2 = false;
                        }
                        if preview_due {
                            if let Err(e) = w.preview(&a) {
                                w.send(Msg::Error(e));
                            }
                        }
                    }
                    Err(e) => {
                        w.cam = None;
                        w.send(Msg::Error(e));
                        next_monitor = Instant::now() + Duration::from_secs(10);
                        next_preview = Instant::now() + Duration::from_secs(2);
                        continue;
                    }
                }
                if monitor_due {
                    let interval = running.unwrap().1;
                    next_monitor = Instant::now()
                        + Duration::from_secs_f64(if fast { FAST_SECS } else { interval });
                }
                if preview_due {
                    next_preview += Duration::from_secs_f64(PREVIEW_SECS);
                    if next_preview < Instant::now() {
                        next_preview = Instant::now();
                    }
                }
                if w.obj
                    .as_ref()
                    .is_some_and(|(_, used)| used.elapsed() > L2_IDLE_DROP)
                {
                    w.obj = None;
                }
            }
        })
        .expect("spawn vision thread");
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    #[test]
    fn embedded_models_load_in_tract() {
        MeshModel::load().unwrap();
        ObjectModel::load().unwrap();
    }

    #[test]
    fn preview_jpeg_is_bounded_and_not_mirrored() {
        let (width, height) = (640, 360);
        let mut bgra = vec![0u8; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let p = &mut bgra[(y * width + x) * 4..];
                if x < width / 2 {
                    p[..4].copy_from_slice(&[0, 0, 255, 255]);
                } else {
                    p[..4].copy_from_slice(&[255, 0, 0, 255]);
                }
            }
        }
        let (jpeg, w, h) = encode_preview(&Frame { width, height, bgra }).unwrap();
        assert_eq!((w, h), (480, 270));
        let decoded = image::load_from_memory(&jpeg).unwrap();
        assert_eq!(decoded.dimensions(), (480, 270));
        let rgb = decoded.to_rgb8();
        let left = rgb.get_pixel(20, 135).0;
        let right = rgb.get_pixel(460, 135).0;
        assert!(left[0] > left[2] && right[2] > right[0]);
    }
}
