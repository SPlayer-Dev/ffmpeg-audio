use std::time::Duration;

pub use pipeline::AudioPipeline;
pub use scan::DurationScanner;
pub use seek::SeekEngine;

use crate::sys;

mod pipeline;
mod scan;
mod seek;

pub struct PlaybackState {
    pub time_base: sys::AVRational,
    pub current_pts: Option<Duration>,
    pub is_exhausted: bool,
    pub has_buffered_seek_frame: bool,
}
