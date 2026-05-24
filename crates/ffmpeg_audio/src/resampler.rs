use std::ptr;

use crate::{
    error::{
        AudioError,
        Result,
    },
    sys,
};

pub struct Resampler {
    ctx: *mut sys::SwrContext,
    out_channels: i32,
}

impl Resampler {
    pub fn new(
        in_layout: &sys::AVChannelLayout,
        in_sample_fmt: sys::AVSampleFormat,
        in_sample_rate: i32,
        target_channels: i32,
        target_sample_rate: i32,
        target_sample_fmt: sys::AVSampleFormat,
    ) -> Result<Self> {
        unsafe {
            if sys::av_sample_fmt_is_planar(target_sample_fmt) == 1 {
                return Err(AudioError::from_ffmpeg(sys::AVERROR_INVALIDDATA));
            }

            let mut out_layout = std::mem::zeroed::<sys::AVChannelLayout>();
            sys::av_channel_layout_default(&raw mut out_layout, target_channels);

            let mut ctx = ptr::null_mut();
            let ret = sys::swr_alloc_set_opts2(
                &raw mut ctx,
                &raw const out_layout,
                target_sample_fmt,
                target_sample_rate,
                in_layout,
                in_sample_fmt,
                in_sample_rate,
                0,
                ptr::null_mut(),
            );
            crate::fferr!(ret);

            if ctx.is_null() {
                return Err(AudioError::from_ffmpeg(sys::AVERROR_ENOMEM));
            }

            let ret = sys::swr_init(ctx);
            if ret < 0 {
                sys::swr_free(&raw mut ctx);
                return Err(AudioError::from_ffmpeg(ret));
            }

            sys::av_channel_layout_uninit(&raw mut out_layout);

            Ok(Self {
                ctx,
                out_channels: target_channels,
            })
        }
    }

    pub fn flush(&mut self) -> Result<()> {
        unsafe {
            let ret = sys::swr_init(self.ctx);
            crate::fferr!(ret);
            Ok(())
        }
    }

    pub fn convert_and_fill(
        &mut self,
        frame: Option<*const sys::AVFrame>,
        out_buffer: &mut Vec<f32>,
    ) -> Result<()> {
        unsafe {
            let (in_data, in_samples) = frame.map_or((ptr::null(), 0), |f| {
                ((*f).extended_data as *const *const u8, (*f).nb_samples)
            });

            let expected_out_samples = sys::swr_get_out_samples(self.ctx, in_samples);
            crate::fferr!(expected_out_samples);

            let needed_capacity = (expected_out_samples * self.out_channels) as usize;

            out_buffer.clear();
            out_buffer.reserve(needed_capacity);

            let out_ptr = out_buffer.as_mut_ptr().cast::<u8>();

            let actual_out_samples = sys::swr_convert(
                self.ctx,
                &raw const out_ptr,
                expected_out_samples,
                in_data,
                in_samples,
            );
            crate::fferr!(actual_out_samples);

            out_buffer.set_len((actual_out_samples * self.out_channels) as usize);

            Ok(())
        }
    }
}

impl Drop for Resampler {
    fn drop(&mut self) {
        unsafe {
            if !self.ctx.is_null() {
                sys::swr_free(&raw mut self.ctx);
            }
        }
    }
}
