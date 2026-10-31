//! Webcam capture through Media Foundation's synchronous source reader. Picks the smallest native
//! mode that is at least 640 px wide (L2 needs the detail for phones) and lets MF convert it to RGB32.

use windows::core::{Result, PWSTR};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::CoTaskMemFree;

pub struct Frame {
    pub width: usize,
    pub height: usize,
    /// Top-down BGRA rows, `width * 4` bytes each.
    pub bgra: Vec<u8>,
}

pub struct Camera {
    reader: IMFSourceReader,
    width: u32,
    height: u32,
    stride: i32,
    last_grab: Option<std::time::Instant>,
}

const STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

pub fn startup() -> Result<()> {
    // SAFETY: plain MF initialisation; balanced by process exit.
    unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE) }
}

fn devices() -> Result<Vec<IMFActivate>> {
    unsafe {
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 1)?;
        let attrs = attrs.unwrap();
        attrs.SetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID)?;
        let mut raw: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        MFEnumDeviceSources(&attrs, &mut raw, &mut count)?;
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            if let Some(a) = (*raw.add(i)).take() {
                out.push(a);
            }
        }
        if !raw.is_null() {
            CoTaskMemFree(Some(raw as *const _));
        }
        Ok(out)
    }
}

pub fn list() -> Vec<String> {
    let Ok(devs) = devices() else { return Vec::new() };
    devs.iter()
        .map(|d| unsafe {
            let mut name = PWSTR::null();
            let mut len = 0u32;
            if d.GetAllocatedString(&MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, &mut name, &mut len).is_ok() {
                let s = name.to_string().unwrap_or_default();
                CoTaskMemFree(Some(name.0 as *const _));
                s
            } else {
                "Camera".to_string()
            }
        })
        .collect()
}

fn frame_size(t: &IMFMediaType) -> Option<(u32, u32)> {
    let v = unsafe { t.GetUINT64(&MF_MT_FRAME_SIZE) }.ok()?;
    Some(((v >> 32) as u32, v as u32))
}

impl Camera {
    pub fn open(index: u32) -> Result<Camera> {
        let devs = devices()?;
        let dev = devs.get(index as usize).or(devs.first()).ok_or_else(|| windows::core::Error::from(MF_E_NOT_FOUND))?;
        unsafe {
            let source: IMFMediaSource = dev.ActivateObject()?;
            let mut attrs = None;
            MFCreateAttributes(&mut attrs, 1)?;
            let attrs = attrs.unwrap();
            attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
            let reader = MFCreateSourceReaderFromMediaSource(&source, &attrs)?;

            // Smallest native mode >= 640 px wide: 320 px frames were too coarse for phone detection.
            let mut best: Option<(u64, IMFMediaType)> = None;
            let mut i = 0;
            while let Ok(t) = reader.GetNativeMediaType(STREAM, i) {
                i += 1;
                if let Some((w, h)) = frame_size(&t) {
                    let area = w as u64 * h as u64;
                    if w >= 640 && best.as_ref().map_or(true, |(a, _)| area < *a) {
                        best = Some((area, t));
                    }
                }
            }
            if let Some((_, t)) = best {
                let _ = reader.SetCurrentMediaType(STREAM, None, &t);
            }

            let out = MFCreateMediaType()?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            reader.SetCurrentMediaType(STREAM, None, &out)?;
            let cur = reader.GetCurrentMediaType(STREAM)?;
            let (width, height) = frame_size(&cur).unwrap_or((640, 480));
            let stride = cur.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|v| v as i32).unwrap_or(width as i32 * 4);
            Ok(Camera { reader, width, height, stride, last_grab: None })
        }
    }

    fn read_sample(&mut self) -> Result<Option<IMFSample>> {
        let mut flags = 0u32;
        let mut sample = None;
        unsafe { self.reader.ReadSample(STREAM, 0, None, Some(&mut flags), None, Some(&mut sample))? };
        if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
            return Err(windows::core::Error::from(MF_E_END_OF_STREAM));
        }
        Ok(sample)
    }

    /// Grab the freshest frame into `frame` (reuses its buffer).
    pub fn grab(&mut self, frame: &mut Frame) -> Result<()> {
        // The first read after an idle gap can be stale. At preview cadence there is no queue to
        // drain, and reading twice would cap a 30 fps camera below the requested ~12 fps preview.
        let reads = if self.last_grab.is_none_or(|t| t.elapsed() > std::time::Duration::from_millis(500)) { 2 } else { 1 };
        let mut sample = None;
        for _ in 0..reads {
            for _ in 0..30 {
                if let Some(s) = self.read_sample()? {
                    sample = Some(s);
                    break;
                }
            }
        }
        self.last_grab = Some(std::time::Instant::now());
        let sample = sample.ok_or_else(|| windows::core::Error::from(MF_E_NO_SAMPLE_TIMESTAMP))?;
        unsafe {
            let buf = sample.ConvertToContiguousBuffer()?;
            let mut ptr = std::ptr::null_mut();
            let mut len = 0u32;
            buf.Lock(&mut ptr, None, Some(&mut len))?;
            let (w, h) = (self.width as usize, self.height as usize);
            let row = w * 4;
            frame.width = w;
            frame.height = h;
            frame.bgra.resize(row * h, 0);
            let src = std::slice::from_raw_parts(ptr, len as usize);
            let pitch = self.stride.unsigned_abs() as usize;
            for y in 0..h {
                // Negative stride = bottom-up rows.
                let sy = if self.stride < 0 { h - 1 - y } else { y };
                let start = sy * pitch;
                if start + row <= src.len() {
                    frame.bgra[y * row..(y + 1) * row].copy_from_slice(&src[start..start + row]);
                }
            }
            buf.Unlock()?;
        }
        Ok(())
    }
}
