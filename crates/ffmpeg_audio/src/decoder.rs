use std::ptr;

use crate::{
    error::{
        AudioError,
        Result,
    },
    sys,
};

pub struct Decoder {
    ctx: *mut sys::AVCodecContext,
    frame: *mut sys::AVFrame,
    is_flushing: bool,
}

impl Decoder {
    pub fn new(codec_params: *mut sys::AVCodecParameters) -> Result<Self> {
        unsafe {
            let codec_id = (*codec_params).codec_id;
            let codec = sys::avcodec_find_decoder(codec_id);
            if codec.is_null() {
                return Err(AudioError::from_ffmpeg(sys::AVERROR_DECODER_NOT_FOUND));
            }

            let mut ctx = sys::avcodec_alloc_context3(codec);
            if ctx.is_null() {
                return Err(AudioError::from_ffmpeg(sys::AVERROR_ENOMEM));
            }

            let ret = sys::avcodec_parameters_to_context(ctx, codec_params);
            if ret < 0 {
                sys::avcodec_free_context(&raw mut ctx);
                return Err(AudioError::from_ffmpeg(ret));
            }

            let ret = sys::avcodec_open2(ctx, codec, ptr::null_mut());
            if ret < 0 {
                sys::avcodec_free_context(&raw mut ctx);
                return Err(AudioError::from_ffmpeg(ret));
            }

            let frame = sys::av_frame_alloc();
            if frame.is_null() {
                sys::avcodec_free_context(&mut ctx.clone());
                return Err(AudioError::from_ffmpeg(sys::AVERROR_ENOMEM));
            }

            Ok(Self {
                ctx,
                frame,
                is_flushing: false,
            })
        }
    }

    pub fn send_packet(&mut self, packet: *const sys::AVPacket) -> Result<()> {
        unsafe {
            let ret = sys::avcodec_send_packet(self.ctx, packet);
            crate::fferr!(ret);
            Ok(())
        }
    }

    pub fn flush(&mut self) {
        unsafe {
            sys::avcodec_flush_buffers(self.ctx);
        }
        self.is_flushing = false;
    }

    pub fn send_eof_flush(&mut self) -> Result<()> {
        if self.is_flushing {
            return Ok(());
        }
        self.is_flushing = true;
        unsafe {
            let ret = sys::avcodec_send_packet(self.ctx, ptr::null());
            crate::fferr!(ret);
        }
        Ok(())
    }

    pub fn receive_frame(&mut self) -> Result<Option<*mut sys::AVFrame>> {
        unsafe {
            sys::av_frame_unref(self.frame);

            let ret = sys::avcodec_receive_frame(self.ctx, self.frame);

            match ret {
                0 => Ok(Some(self.frame)),
                crate::sys::AVERROR_EAGAIN => Err(AudioError::Eagain),
                crate::sys::AVERROR_EOF => Ok(None),
                _ => Err(AudioError::from_ffmpeg(ret)),
            }
        }
    }

    pub fn sample_rate(&self) -> i32 {
        unsafe { (*self.ctx).sample_rate }
    }

    pub fn channels(&self) -> i32 {
        unsafe { (*self.ctx).ch_layout.nb_channels }
    }

    pub fn sample_fmt(&self) -> sys::AVSampleFormat {
        unsafe { (*self.ctx).sample_fmt }
    }

    pub fn channel_layout(&self) -> sys::AVChannelLayout {
        unsafe {
            let mut layout = (*self.ctx).ch_layout;

            if layout.order == sys::AVChannelOrder_AV_CHANNEL_ORDER_UNSPEC && layout.nb_channels > 0
            {
                sys::av_channel_layout_default(&raw mut layout, layout.nb_channels);
            }
            layout
        }
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        unsafe {
            if !self.frame.is_null() {
                sys::av_frame_free(&raw mut self.frame);
            }
            if !self.ctx.is_null() {
                sys::avcodec_free_context(&raw mut self.ctx);
            }
        }
    }
}
