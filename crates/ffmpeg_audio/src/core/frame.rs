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
    timeline_origin_pts: i64,
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
            timeline_origin_pts: 0,
            sample_offset: 0,
            _marker: PhantomData,
        }
    }

    /// Injects a sample offset for sample-accurate seeking.
    pub(crate) const fn with_offset(mut self, offset: usize) -> Self {
        self.sample_offset = offset;
        self
    }

    /// Defines the stream timestamp that corresponds to the public timeline origin.
    pub(crate) const fn with_timeline_origin(mut self, origin_pts: i64) -> Self {
        self.timeline_origin_pts = origin_pts;
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

    /// Returns the PTS in microseconds relative to the stream timeline origin, if available.
    pub(crate) fn pts_micros(&self) -> Option<i64> {
        let raw_pts = unsafe { (*self.ptr.as_ptr()).pts };
        if raw_pts == sys::AV_NOPTS_VALUE {
            return None;
        }
        let relative_pts = raw_pts.saturating_sub(self.timeline_origin_pts);

        self.time_base.calc_micros(relative_pts).map(|mut micros| {
            let sample_rate = self.frame_sample_rate();

            if self.sample_offset > 0 && sample_rate > 0 {
                let offset_micros =
                    (self.sample_offset as i64 * 1_000_000) / i64::from(sample_rate);
                micros = micros.saturating_add(offset_micros);
            }
            micros
        })
    }

    /// Returns the Presentation Timestamp (PTS) of this frame relative to the stream timeline
    /// origin, if available.
    ///
    /// The timestamp is automatically adjusted forward by the internal sample offset.
    ///
    /// # Returns
    /// - `Some(Duration)` representing the exact playback time of the frame.
    /// - `None` if the underlying frame lacks a valid PTS (`AV_NOPTS_VALUE`).
    #[must_use]
    pub fn pts(&self) -> Option<Duration> {
        let raw_pts = unsafe { (*self.ptr.as_ptr()).pts };
        if raw_pts == sys::AV_NOPTS_VALUE {
            return None;
        }
        let relative_pts = raw_pts.saturating_sub(self.timeline_origin_pts);

        self.time_base
            .calc_duration(relative_pts)
            .map(|mut duration| {
                let sample_rate = self.frame_sample_rate();

                if self.sample_offset > 0 && sample_rate > 0 {
                    let offset_secs = self.sample_offset as f64 / f64::from(sample_rate);
                    duration += Duration::from_secs_f64(offset_secs);
                }
                duration
            })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        mem,
        time::Duration,
    };

    use super::*;

    #[test]
    fn pts_is_relative_to_the_timeline_origin() {
        let mut raw_frame = unsafe { mem::zeroed::<sys::AVFrame>() };
        raw_frame.pts = 10_000_000;
        raw_frame.nb_samples = 1_024;
        raw_frame.sample_rate = 48_000;

        let time_base = TimeBase::try_new(sys::AVRational {
            num: 1,
            den: 1_000_000,
        })
        .unwrap();

        let frame = AudioFrame::new(&raw const raw_frame, time_base)
            .with_timeline_origin(10_000_000)
            .with_offset(48);

        assert_eq!(frame.pts_micros(), Some(1_000));
        assert_eq!(frame.pts(), Some(Duration::from_micros(1_000)));

        raw_frame.pts = sys::AV_NOPTS_VALUE;
        let no_pts_frame =
            AudioFrame::new(&raw const raw_frame, time_base).with_timeline_origin(-1);
        assert_eq!(no_pts_frame.pts_micros(), None);
        assert_eq!(no_pts_frame.pts(), None);
    }
}
