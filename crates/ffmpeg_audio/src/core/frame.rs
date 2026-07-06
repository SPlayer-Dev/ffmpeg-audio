use std::{
    marker::PhantomData,
    ptr::NonNull,
    time::Duration,
};

use crate::{
    TimeBase,
    sys,
};

/// A safe, zero-copy wrapper around FFmpeg's raw `AVFrame`.
///
/// This wrapper is useful for 1-to-N zero-copy dispatching to multiple downstream
/// `Resampler` instances simultaneously.
pub struct AudioFrame<'a> {
    ptr: NonNull<sys::AVFrame>,
    time_base: TimeBase,
    sample_offset: usize,
    _marker: PhantomData<&'a mut ()>,
}

impl AudioFrame<'_> {
    /// Creates a new `AudioFrame` wrapper.
    ///
    /// # Safety
    /// This method is for internal crate use. The caller ensures that the provided
    /// `ptr` is a valid FFmpeg `AVFrame` and that its memory remains valid for the
    /// duration of the lifetime.
    pub(crate) const fn new(ptr: *const sys::AVFrame, time_base: TimeBase) -> Self {
        Self {
            ptr: NonNull::new(ptr.cast_mut()).expect("FFmpeg returned a null AVFrame pointer"),
            time_base,
            sample_offset: 0,
            _marker: PhantomData,
        }
    }

    /// Injects a sample offset for sample-accurate seeking.
    pub(crate) const fn with_offset(mut self, offset: usize) -> Self {
        self.sample_offset = offset;
        self
    }

    /// Extracts the underlying raw FFmpeg `AVFrame` pointer.
    ///
    /// This is used internally to pass the raw frame data into FFmpeg's FFI functions
    /// (such as the resampling context).
    pub(crate) const fn as_ptr(&self) -> *const sys::AVFrame {
        self.ptr.as_ptr()
    }

    /// Returns the number of available audio samples (per channel) contained in this frame.
    ///
    /// For example, if a stereo frame contains 1024 samples and has an offset of 100,
    /// this will return `924`.
    #[must_use]
    pub const fn samples(&self) -> usize {
        let raw_samples = unsafe { (*self.ptr.as_ptr()).nb_samples as usize };
        raw_samples.saturating_sub(self.sample_offset)
    }

    /// Returns the offset applied to the beginning of the frame's payload.
    pub(crate) const fn offset(&self) -> usize {
        self.sample_offset
    }

    /// Returns the actual sample format of this specific frame.
    pub(crate) fn sample_fmt(&self) -> sys::AVSampleFormat {
        unsafe { (*self.ptr.as_ptr()).format }
    }

    /// Returns a reference to the actual channel layout of this specific frame.
    pub(crate) fn channel_layout(&self) -> &sys::AVChannelLayout {
        unsafe { &(*self.ptr.as_ptr()).ch_layout }
    }

    /// Returns the actual sample rate of this specific frame.
    pub(crate) fn frame_sample_rate(&self) -> i32 {
        unsafe { (*self.ptr.as_ptr()).sample_rate }
    }

    /// Returns the Presentation Timestamp (PTS) of this frame, if available.
    ///
    /// The timestamp is automatically adjusted forward by the internal sample offset.
    ///
    /// # Returns
    /// - `Some(Duration)` representing the exact playback time of the frame.
    /// - `None` if the underlying frame lacks a valid PTS (`AV_NOPTS_VALUE`).
    #[must_use]
    pub fn pts(&self) -> Option<Duration> {
        let raw_pts = unsafe { (*self.ptr.as_ptr()).pts };

        self.time_base.calc_duration(raw_pts).map(|mut duration| {
            let sample_rate = self.frame_sample_rate();

            if self.sample_offset > 0 && sample_rate > 0 {
                let offset_secs = self.sample_offset as f64 / f64::from(sample_rate);
                duration += Duration::from_secs_f64(offset_secs);
            }
            duration
        })
    }
}
