use std::time::Duration;

pub use pipeline::AudioPipeline;
pub use scan::DurationScanner;
pub use seek::SeekEngine;

use crate::time::TimeBase;

mod pipeline;
mod scan;
mod seek;

pub struct PlaybackState {
    pub time_base: TimeBase,
    pub current_pts: Option<Duration>,
    pub is_exhausted: bool,
    pub has_buffered_seek_frame: bool,
}

impl PlaybackState {
    pub fn debug_verify(&self) {
        debug_assert!(
            !(self.is_exhausted && self.has_buffered_seek_frame),
            "Stream is marked as exhausted, but a buffered seek frame is present."
        );

        let tb = self.time_base.as_rational();

        debug_assert!(tb.den > 0, "Time base denominator is zero or negative.");

        debug_assert!(tb.num > 0, "Time base numerator is zero or negative.");
    }
}
