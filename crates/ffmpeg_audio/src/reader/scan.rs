use std::time::Duration;

use crate::{
    SourceAudioInfo,
    decoder::Decoder,
    demuxer::Demuxer,
    error::Result,
    reader::{
        AudioPipeline,
        PlaybackState,
        SeekEngine,
    },
    sys,
};

pub struct DurationScanner;

impl DurationScanner {
    pub fn scan_exact(
        demuxer: &mut Demuxer,
        decoder: &mut Decoder,
        state: &mut PlaybackState,
        source_info: &SourceAudioInfo,
        fast_mode: bool,
    ) -> Result<Option<Duration>> {
        let original_position = state.current_pts.unwrap_or(Duration::ZERO);

        SeekEngine::seek_to(demuxer, decoder, state, source_info, Duration::ZERO)?;

        let mut max_pts_us: Option<i64> = None;
        let mut last_frame_duration_us: i64 = 0;
        let mut total_samples_fallback: usize = 0;
        let mut scan_error = None;

        if fast_mode {
            loop {
                match demuxer.read_packet() {
                    Ok(Some(packet)) => unsafe {
                        let pts = (*packet).pts;
                        if pts != sys::AV_NOPTS_VALUE {
                            let duration = (*packet).duration;
                            let end_pts = if duration > 0 {
                                pts.saturating_add(duration)
                            } else {
                                pts
                            };
                            let bq = sys::AVRational {
                                num: 1,
                                den: sys::AV_TIME_BASE.cast_signed(),
                            };
                            let end_us = sys::av_rescale_q(end_pts, state.time_base, bq);
                            max_pts_us = Some(max_pts_us.unwrap_or(0).max(end_us));
                        }
                    },
                    Ok(None) => break,
                    Err(e) => {
                        scan_error = Some(e);
                        break;
                    }
                }
            }
        } else {
            let sample_rate = f64::from(source_info.sample_rate);
            loop {
                match AudioPipeline::receive_frame(demuxer, decoder, state) {
                    Ok(Some(frame)) => {
                        let samples = frame.samples();
                        total_samples_fallback += samples;
                        if let Some(pts) = frame.pts() {
                            max_pts_us = Some(max_pts_us.unwrap_or(0).max(pts.as_micros() as i64));
                        }
                        if sample_rate > 0.0 {
                            let duration_secs = samples as f64 / sample_rate;
                            last_frame_duration_us = (duration_secs * 1_000_000.0) as i64;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        scan_error = Some(e);
                        break;
                    }
                }
            }
        }

        SeekEngine::seek_to(demuxer, decoder, state, source_info, original_position)?;

        if let Some(e) = scan_error {
            return Err(e);
        }

        max_pts_us.map_or_else(
            || {
                // The entire file contains no PTS, yet we successfully decoded the sample waveform
                // We calculate the absolute duration by dividing the total number of physical samples
                // by the sampling rate
                if !fast_mode && total_samples_fallback > 0 && source_info.sample_rate > 0 {
                    let duration_secs =
                        total_samples_fallback as f64 / f64::from(source_info.sample_rate);
                    Ok(Some(Duration::from_secs_f64(duration_secs)))
                } else {
                    // Empty file, or no timestamped packets found in Fast Mode
                    Ok(None)
                }
            },
            |pts| {
                // located a valid timestamp, and add the physical duration of the final frame
                let total_us = pts.saturating_add(last_frame_duration_us);
                Ok(Some(Duration::from_micros(total_us.max(0).cast_unsigned())))
            },
        )
    }
}
