use crate::{
    AudioError,
    AudioFrame,
    decoder::Decoder,
    demuxer::Demuxer,
    error::Result,
    reader::PlaybackState,
};

pub struct AudioPipeline;

impl AudioPipeline {
    pub fn receive_frame<'a>(
        demuxer: &mut Demuxer,
        decoder: &'a mut Decoder,
        state: &mut PlaybackState,
    ) -> Result<Option<AudioFrame<'a>>> {
        if state.is_exhausted {
            return Ok(None);
        }

        if state.has_buffered_seek_frame {
            state.has_buffered_seek_frame = false;
            let frame_ptr = decoder.current_frame();
            let audio_frame = AudioFrame::new(frame_ptr, state.time_base);
            state.current_pts = audio_frame.pts();
            return Ok(Some(audio_frame));
        }

        loop {
            match decoder.receive_frame() {
                Ok(Some(frame)) => {
                    let audio_frame = AudioFrame::new(frame, state.time_base);
                    state.current_pts = audio_frame.pts();
                    return Ok(Some(audio_frame));
                }
                Err(AudioError::Eagain) => match demuxer.read_packet()? {
                    Some(packet) => decoder.send_packet(packet)?,
                    None => decoder.send_eof_flush()?,
                },
                Ok(None) => {
                    state.is_exhausted = true;
                    return Ok(None);
                }
                Err(e) => return Err(e),
            }
        }
    }
}
