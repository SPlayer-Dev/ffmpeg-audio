use std::time::Duration;

use crate::{
    SourceAudioInfo,
    decoder::Decoder,
    demuxer::Demuxer,
    error::Result,
    reader::{
        AudioPipeline,
        PlaybackState,
    },
};
pub struct SeekEngine;

impl SeekEngine {
    pub fn seek_to(
        demuxer: &mut Demuxer,
        decoder: &mut Decoder,
        state: &mut PlaybackState,
        source_info: &SourceAudioInfo,
        target: Duration,
    ) -> Result<()> {
        state.debug_verify();

        demuxer.seek_to(target)?;
        decoder.flush();

        state.is_exhausted = false;
        state.current_pts = None;
        state.has_buffered_seek_frame = false;

        let target_us = target.as_micros() as i64;
        let sample_rate = f64::from(source_info.sample_rate);

        // Decode and discard frames until reaching the target
        // Prevents ffmpeg from jumping to the keyframe preceding the target position,
        // avoiding playback lag.
        loop {
            match AudioPipeline::receive_frame(demuxer, decoder, state) {
                Ok(Some(frame)) => {
                    if let Some(pts) = frame.pts() {
                        let pts_us = pts.as_micros() as i64;
                        let duration_us = if sample_rate > 0.0 {
                            (frame.samples() as f64 / sample_rate * 1_000_000.0) as i64
                        } else {
                            0
                        };

                        // If the end time of the current frame surpasses the target timestamp,
                        // this frame contains or immediately follows the exact target playback position
                        if pts_us + duration_us >= target_us {
                            // Buffer this frame so it can be immediately consumed by the next
                            // external call to `receive_frame`
                            state.has_buffered_seek_frame = true;
                            break;
                        }
                    } else {
                        // If a frame lacks a valid PTS, stop discarding immediately
                        // to prevent accidentally consuming and exhausting the entire stream.
                        state.has_buffered_seek_frame = true;
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => return Err(e),
            }
        }

        // Eliminate the PTS update side effects resulting from calling `receive_frame` within
        // the decoding loop
        state.current_pts = None;

        state.debug_verify();
        Ok(())
    }
}
