use crate::sys;

/// A trait binding Rust native numeric types to FFmpeg's `AVSampleFormat`.
///
/// This trait is used to ensure type safety when extracting resampled audio data.
pub trait AudioSample: Copy + Send + Sync + 'static {
    /// The FFmpeg sample format enum corresponding to the packed (interleaved) layout.
    const PACKED_FORMAT: sys::AVSampleFormat;

    /// The FFmpeg sample format enum corresponding to the planar layout.
    const PLANAR_FORMAT: sys::AVSampleFormat;
}

impl AudioSample for f32 {
    const PACKED_FORMAT: sys::AVSampleFormat = sys::AVSampleFormat_AV_SAMPLE_FMT_FLT;
    const PLANAR_FORMAT: sys::AVSampleFormat = sys::AVSampleFormat_AV_SAMPLE_FMT_FLTP;
}

impl AudioSample for i16 {
    const PACKED_FORMAT: sys::AVSampleFormat = sys::AVSampleFormat_AV_SAMPLE_FMT_S16;
    const PLANAR_FORMAT: sys::AVSampleFormat = sys::AVSampleFormat_AV_SAMPLE_FMT_S16P;
}

impl AudioSample for i32 {
    #[expect(clippy::use_self)]
    const PACKED_FORMAT: sys::AVSampleFormat = sys::AVSampleFormat_AV_SAMPLE_FMT_S32;
    #[expect(clippy::use_self)]
    const PLANAR_FORMAT: sys::AVSampleFormat = sys::AVSampleFormat_AV_SAMPLE_FMT_S32P;
}

impl AudioSample for u8 {
    const PACKED_FORMAT: sys::AVSampleFormat = sys::AVSampleFormat_AV_SAMPLE_FMT_U8;
    const PLANAR_FORMAT: sys::AVSampleFormat = sys::AVSampleFormat_AV_SAMPLE_FMT_U8P;
}
