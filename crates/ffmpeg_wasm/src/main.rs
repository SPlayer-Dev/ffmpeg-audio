mod io;

use std::{
    ffi::{
        CString,
        c_char,
    },
    ptr,
    time::Duration,
};

use ffmpeg_audio::{
    AudioReader,
    ResampleOptions,
    ResampledReader,
    SeekMode,
};
use io::JsFileAccess;

pub struct DecoderContext {
    reader: ResampledReader,
    current_samples: usize,
    current_ptrs: Vec<*const f32>,

    metadata_json: CString,
    cover_data: Vec<u8>,
    cover_mime: Option<CString>,

    compute_peaks: bool,
    frame_min: f32,
    frame_max: f32,
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_decoder_create(
    file_id: u32,
    target_sample_rate: i32,
    target_channels: i32,
) -> *mut DecoderContext {
    let file_access = match JsFileAccess::new(file_id) {
        Ok(acc) => acc,
        Err(e) => {
            println!("[ffmpeg_wasm] JsFileAccess Error: {e:?}");
            return ptr::null_mut();
        }
    };

    let reader = match AudioReader::new(file_access) {
        Ok(r) => r,
        Err(e) => {
            println!("[ffmpeg_wasm] AudioReader::new Error: {e:?}");
            return ptr::null_mut();
        }
    };

    let metadata_map = reader.metadata();
    let metadata_json = match serde_json::to_string(&metadata_map) {
        Ok(json) => CString::new(json).unwrap_or_default(),
        Err(e) => {
            println!("[ffmpeg_wasm] Metadata JSON Error: {e:?}");
            CString::default()
        }
    };

    let cover = reader.cover();
    let cover_data = cover.as_ref().map_or_else(Vec::new, |c| c.data.clone());
    let cover_mime = cover
        .and_then(|c| c.mime_type)
        .and_then(|m| CString::new(m).ok());

    let options = ResampleOptions::new()
        .sample_rate(target_sample_rate)
        .channels(target_channels)
        .format_planar::<f32>();
    let resampled_reader = match reader.into_resampled(options) {
        Ok(rr) => rr,
        Err(e) => {
            println!("[ffmpeg_wasm] Resampler Error: {e:?}");
            return ptr::null_mut();
        }
    };

    let ctx = Box::new(DecoderContext {
        reader: resampled_reader,
        current_samples: 0,
        current_ptrs: Vec::with_capacity(target_channels as usize),
        metadata_json,
        cover_data,
        cover_mime,
        compute_peaks: false,
        frame_min: 0.0,
        frame_max: 0.0,
    });
    Box::into_raw(ctx)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_decoder_destroy(ctx_ptr: *mut DecoderContext) {
    if !ctx_ptr.is_null() {
        unsafe {
            let _ = Box::from_raw(ctx_ptr);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_decoder_decode_frame(ctx_ptr: *mut DecoderContext) -> i32 {
    let ctx = unsafe { &mut *ctx_ptr };

    loop {
        match ctx.reader.receive_planar_as::<f32>() {
            Ok(Some(channels_data)) => {
                if channels_data.is_empty() {
                    continue;
                }

                let len = channels_data[0].len();
                if len == 0 {
                    continue;
                }

                ctx.current_samples = len;
                ctx.current_ptrs = channels_data.iter().map(|slice| slice.as_ptr()).collect();

                if ctx.compute_peaks {
                    let ch0 = channels_data[0];
                    let (mut min_val, mut max_val) = (ch0[0], ch0[0]);

                    for &val in ch0 {
                        if val < min_val {
                            min_val = val;
                        }
                        if val > max_val {
                            max_val = val;
                        }
                    }
                    ctx.frame_min = min_val;
                    ctx.frame_max = max_val;
                } else {
                    ctx.frame_min = 0.0;
                    ctx.frame_max = 0.0;
                }

                return 1;
            }
            Ok(None) => return 0,
            Err(_) => return -1,
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_decoder_set_compute_peaks(ctx_ptr: *mut DecoderContext, enable: i32) {
    let ctx = unsafe { &mut *ctx_ptr };
    ctx.compute_peaks = enable != 0;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_decoder_get_frame_min(ctx_ptr: *mut DecoderContext) -> f32 {
    let ctx = unsafe { &mut *ctx_ptr };
    ctx.frame_min
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_decoder_get_frame_max(ctx_ptr: *mut DecoderContext) -> f32 {
    let ctx = unsafe { &mut *ctx_ptr };
    ctx.frame_max
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_decoder_get_frame_samples(ctx_ptr: *mut DecoderContext) -> u32 {
    let ctx = unsafe { &mut *ctx_ptr };
    ctx.current_samples as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_decoder_get_channel_ptr(
    ctx_ptr: *mut DecoderContext,
    channel: u32,
) -> *const f32 {
    let ctx = unsafe { &mut *ctx_ptr };
    let ch = channel as usize;
    if ch < ctx.current_ptrs.len() {
        ctx.current_ptrs[ch]
    } else {
        ptr::null()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_decoder_seek(
    ctx_ptr: *mut DecoderContext,
    target_seconds: f64,
) -> i32 {
    if target_seconds < 0.0 {
        return -1;
    }

    let ctx = unsafe { &mut *ctx_ptr };
    let duration = Duration::from_secs_f64(target_seconds);

    if ctx.reader.seek(duration, SeekMode::Accurate).is_ok() {
        ctx.current_samples = 0;
        ctx.current_ptrs.clear();
        1
    } else {
        -1
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_decoder_get_duration(ctx_ptr: *mut DecoderContext) -> f64 {
    let ctx = unsafe { &mut *ctx_ptr };
    ctx.reader
        .source()
        .duration()
        .map_or_else(|| -1.0, |d| d.as_secs_f64())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_decoder_get_metadata_json(
    ctx_ptr: *const DecoderContext,
) -> *const c_char {
    let ctx = unsafe { &*ctx_ptr };
    ctx.metadata_json.as_ptr()
}

#[unsafe(no_mangle)]
pub const unsafe extern "C" fn wasm_decoder_get_cover_ptr(
    ctx_ptr: *const DecoderContext,
) -> *const u8 {
    let ctx = unsafe { &*ctx_ptr };
    if ctx.cover_data.is_empty() {
        ptr::null()
    } else {
        ctx.cover_data.as_ptr()
    }
}

#[unsafe(no_mangle)]
pub const unsafe extern "C" fn wasm_decoder_get_cover_size(ctx_ptr: *const DecoderContext) -> u32 {
    let ctx = unsafe { &*ctx_ptr };
    ctx.cover_data.len() as u32
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn wasm_decoder_get_cover_mime(
    ctx_ptr: *const DecoderContext,
) -> *const c_char {
    let ctx = unsafe { &*ctx_ptr };
    ctx.cover_mime.as_ref().map_or(ptr::null(), |m| m.as_ptr())
}

const fn main() {}
