use std::{
    io::{
        Read,
        Seek,
    },
    time::Duration,
};

use crate::{
    AudioError,
    AudioFrame,
    Decoder,
    Demuxer,
    Result,
    TimeBase,
    decode::io::IoContext,
    sys,
};

/// Specifies the strategy used to scan an audio stream to determine its exact duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    /// Rapidly scans the stream by reading demuxer packet timestamps without decoding them.
    ///
    /// This mode is extremely fast and relies entirely on the container's metadata.
    /// However, it may fail or return inaccurate results for raw formats or highly
    /// corrupted streams that lack valid timestamp information.
    Packet,

    /// Fully decodes the stream into raw physical audio frames to calculate the duration.
    ///
    /// This mode is the most accurate fallback method, as it calculates time based purely
    /// on the actual number of generated audio samples and the stream's sample rate.
    /// Because it requires full decompression, it consumes significantly more CPU
    /// and takes much longer to complete.
    Frame,
}

/// A decode engine that orchestrates the extraction and decoding of audio data.
///
/// This engine acts as a unified abstraction over FFmpeg's underlying parsing (`Demuxer`)
/// and decompression (`Decoder`) stages. It encapsulates the complex send/receive
/// state machines, timestamp alignments, and buffering required to safely yield raw audio frames.
pub struct DecodeEngine {
    /// The underlying component responsible for reading and parsing the media container.
    demuxer: Demuxer,

    /// The underlying component responsible for decompressing raw packets into audio frames.
    decoder: Decoder,

    /// The fundamental unit of time representation for the current stream.
    time_base: TimeBase,

    /// The presentation timestamp (PTS) of the most recently decoded frame, if available.
    current_pts: Option<Duration>,

    /// Indicates whether the internal stream has reached the End Of File (EOF).
    is_exhausted: bool,

    /// Indicates whether a valid frame was decoded and buffered during a seek operation,
    /// waiting to be consumed by the next read invocation.
    has_buffered_seek_frame: bool,
}

impl DecodeEngine {
    /// Constructs a new `DecodeEngine` by taking full ownership of a demuxer and a decoder.
    ///
    /// # Arguments
    /// * `demuxer` - A fully initialized demuxer pointing to a valid audio stream.
    /// * `decoder` - A fully initialized decoder configured with the matching codec parameters.
    ///
    /// # Returns
    /// * `Ok(DecodeEngine)` if the engine is successfully initialized and the time base is valid.
    /// * `Err(AudioError)` if the demuxer fails to provide a valid time base for synchronization.
    pub fn new<T>(source: T) -> Result<Self>
    where
        T: Read + Seek + Send + 'static,
    {
        let io_ctx = IoContext::new(source)?;

        let demuxer = Demuxer::new(io_ctx)?;

        let codec_params = demuxer.stream_codec_params();
        let decoder = Decoder::new(codec_params)?;

        let time_base = demuxer.time_base()?;

        Ok(Self {
            demuxer,
            decoder,
            time_base,
            current_pts: None,
            is_exhausted: false,
            has_buffered_seek_frame: false,
        })
    }

    fn debug_verify(&self) {
        debug_assert!(
            !(self.is_exhausted && self.has_buffered_seek_frame),
            "Stream is marked as exhausted, but a buffered seek frame is present."
        );

        let tb = self.time_base.as_rational();

        debug_assert!(tb.den > 0, "Time base denominator is zero or negative.");

        debug_assert!(tb.num > 0, "Time base numerator is zero or negative.");
    }

    /// Pulls and decodes the next available audio frame from the underlying stream.
    ///
    /// # Returns
    /// * `Ok(Some(AudioFrame))` containing the decompressed audio data ready for consumption.
    /// * `Ok(None)` if the stream has reached the End Of File (EOF).
    /// * `Err(AudioError)` if an I/O failure or a fatal FFmpeg decoding error occurs.
    pub fn receive_frame(&mut self) -> Result<Option<AudioFrame<'_>>> {
        self.debug_verify();

        if self.is_exhausted {
            return Ok(None);
        }

        if self.has_buffered_seek_frame {
            self.has_buffered_seek_frame = false;
            let frame_ptr = self.decoder.current_frame();
            let audio_frame = AudioFrame::new(frame_ptr, self.time_base);
            self.current_pts = audio_frame.pts();

            self.debug_verify();
            return Ok(Some(audio_frame));
        }

        loop {
            match self.decoder.receive_frame() {
                Ok(Some(frame)) => {
                    let audio_frame = AudioFrame::new(frame, self.time_base);
                    self.current_pts = audio_frame.pts();

                    self.debug_verify();
                    return Ok(Some(audio_frame));
                }
                Err(AudioError::Eagain) => match self.demuxer.read_packet()? {
                    Some(packet) => self.decoder.send_packet(packet)?,
                    None => self.decoder.send_eof_flush()?,
                },
                Ok(None) => {
                    self.is_exhausted = true;
                    self.debug_verify();
                    return Ok(None);
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// Seeks the underlying audio stream to the specified target presentation time.
    ///
    /// # Arguments
    /// * `target` - The exact chronological point in the audio stream to seek to.
    ///
    /// # Errors
    /// Returns an `AudioError` if the underlying demuxer fails to seek, or if a decoding
    /// error occurs during the frame alignment process.
    pub fn seek(&mut self, target: Duration) -> Result<()> {
        self.debug_verify();

        self.demuxer.seek_to(target)?;
        self.decoder.flush();

        self.is_exhausted = false;
        self.current_pts = None;
        self.has_buffered_seek_frame = false;

        let target_us = target.as_micros() as i64;
        let sample_rate = f64::from(self.decoder.sample_rate());

        // Decode and discard frames until reaching the target
        // Prevents ffmpeg from jumping to the keyframe preceding the target position,
        // avoiding playback lag.
        loop {
            match self.receive_frame() {
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
                            self.has_buffered_seek_frame = true;
                            break;
                        }
                    } else {
                        // If a frame lacks a valid PTS, stop discarding immediately
                        // to prevent accidentally consuming and exhausting the entire stream.
                        self.has_buffered_seek_frame = true;
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => return Err(e),
            }
        }

        // Eliminate the PTS update side effects resulting from calling `receive_frame` within
        // the decoding loop
        self.current_pts = None;

        self.debug_verify();
        Ok(())
    }

    /// Scans the audio stream to determine its exact total duration.
    ///
    /// This operation performs internal seeking and state resets. It is recommended to
    /// call this method before establishing a continuous reading pipeline to prevent
    /// disrupting the primary playback flow.
    ///
    /// # Arguments
    /// * `mode` - The strategy ([`ScanMode`]) to employ during the scanning process.
    ///
    /// # Returns
    /// * `Ok(Some(Duration))` representing the accurate total length of the audio stream.
    /// * `Ok(None)` if the file is completely empty or lacks valid timestamp data.
    /// * `Err(AudioError)` if an I/O or parsing failure halts the scanning process.
    pub fn scan_duration(&mut self, mode: ScanMode) -> Result<Option<Duration>> {
        let original_position = if self.has_buffered_seek_frame {
            let frame_ptr = self.decoder.current_frame();
            AudioFrame::new(frame_ptr, self.time_base).pts()
        } else {
            self.current_pts
        }
        .unwrap_or(Duration::ZERO);

        self.seek(Duration::ZERO)?;

        let mut max_pts_us: Option<i64> = None;
        let mut last_frame_duration_us: i64 = 0;
        let mut total_samples_fallback: usize = 0;
        let mut scan_error = None;

        match mode {
            ScanMode::Packet => loop {
                match self.demuxer.read_packet() {
                    Ok(Some(packet)) => unsafe {
                        let pts = (*packet).pts;
                        if pts == sys::AV_NOPTS_VALUE {
                            continue;
                        }
                        let duration = (*packet).duration;
                        let end_pts = if duration > 0 {
                            pts.saturating_add(duration)
                        } else {
                            pts
                        };
                        if let Some(end_us) = self.time_base.calc_micros(end_pts) {
                            max_pts_us = Some(max_pts_us.unwrap_or(0).max(end_us));
                        }
                    },
                    Ok(None) => break,
                    Err(e) => {
                        scan_error = Some(e);
                        break;
                    }
                }
            },
            ScanMode::Frame => {
                let sample_rate = f64::from(self.decoder.sample_rate());
                loop {
                    match self.receive_frame() {
                        Ok(Some(frame)) => {
                            let samples = frame.samples();
                            total_samples_fallback += samples;
                            if let Some(pts) = frame.pts() {
                                max_pts_us =
                                    Some(max_pts_us.unwrap_or(0).max(pts.as_micros() as i64));
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
        }

        self.seek(original_position)?;

        if let Some(e) = scan_error {
            return Err(e);
        }

        max_pts_us.map_or_else(
            || {
                let sample_rate = self.decoder.sample_rate();
                if mode == ScanMode::Frame && total_samples_fallback > 0 && sample_rate > 0 {
                    let duration_secs = total_samples_fallback as f64 / f64::from(sample_rate);
                    Ok(Some(Duration::from_secs_f64(duration_secs)))
                } else {
                    Ok(None)
                }
            },
            |pts| {
                let total_us = pts.saturating_add(last_frame_duration_us);
                Ok(Some(Duration::from_micros(total_us.max(0).cast_unsigned())))
            },
        )
    }

    /// Returns a shared, immutable reference to the underlying demuxer.
    pub(crate) const fn demuxer(&self) -> &Demuxer {
        &self.demuxer
    }

    /// Returns a shared, immutable reference to the underlying decoder.
    pub(crate) const fn decoder(&self) -> &Decoder {
        &self.decoder
    }

    /// Returns the presentation timestamp of the most recently decoded audio frame.
    ///
    /// # Returns
    /// * `Some(Duration)` representing the current playback position.
    /// * `None` if no frames have been successfully decoded yet, or immediately after a seek.
    pub const fn stream_position(&self) -> Option<Duration> {
        self.current_pts
    }
}
