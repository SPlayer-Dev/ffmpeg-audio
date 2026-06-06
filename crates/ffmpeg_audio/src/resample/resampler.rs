use std::{
    mem::{
        self,
        MaybeUninit,
    },
    ptr,
};

use crate::{
    AudioFrame,
    core::format::AudioSample,
    error::{
        AudioError,
        Result,
    },
    resample::SwrContext,
    sys,
};

/// Configuration options for the audio resampler.
///
/// Use the builder pattern to construct resampling parameters such as
/// target sample rate, number of channels, and audio data format.
#[derive(Debug, Clone)]
pub struct ResampleOptions {
    pub target_sample_rate: i32,
    pub target_channels: i32,
    pub target_sample_fmt: sys::AVSampleFormat,
}

impl Default for ResampleOptions {
    fn default() -> Self {
        Self {
            target_sample_rate: 44100,
            target_channels: 2,
            target_sample_fmt: sys::AVSampleFormat_AV_SAMPLE_FMT_FLT,
        }
    }
}

impl ResampleOptions {
    /// Creates a new [`ResampleOptions`] builder with default settings
    /// (44100 Hz, Stereo, 32-bit Float).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<()> {
        if self.target_sample_rate <= 0 {
            return Err(AudioError::InvalidParameter(
                "Target sample rate must be greater than 0".to_string(),
            ));
        }
        if self.target_channels <= 0 {
            return Err(AudioError::InvalidParameter(
                "Target channels must be greater than 0".to_string(),
            ));
        }
        Ok(())
    }

    /// Sets the target sample rate (in Hz).
    #[must_use]
    pub const fn sample_rate(mut self, rate: i32) -> Self {
        self.target_sample_rate = rate;
        self
    }

    /// Sets the target number of audio channels.
    ///
    /// For example, `1` for Mono, `2` for Stereo.
    #[must_use]
    pub const fn channels(mut self, channels: i32) -> Self {
        self.target_channels = channels;
        self
    }

    /// Sets the target audio sample format as Packed (Interleaved).
    ///
    /// For example, `[L, R, L, R]`
    #[must_use]
    pub const fn format<T: AudioSample>(mut self) -> Self {
        self.target_sample_fmt = T::PACKED_FORMAT;
        self
    }

    /// Sets the target audio sample format as Planar.
    ///
    /// For example, `[L, L, L]` and `[R, R, R]` in separate continuous blocks.
    #[must_use]
    pub const fn format_planar<T: AudioSample>(mut self) -> Self {
        self.target_sample_fmt = T::PLANAR_FORMAT;
        self
    }
}

/// The high-level audio resampler pipeline.
///
/// This struct manages the format verification, buffer allocation, and
/// interaction with the underlying FFmpeg `SwrContext`. It is strictly
/// non-generic to prevent generic viral spread, applying type parameters
/// only at the boundaries of data processing (`process`) and extraction (`output_as`).
pub struct Resampler {
    swr: SwrContext,
    options: ResampleOptions,
    buffer: RawAudioBuffer,
    actual_samples_per_channel: usize,
    stride_samples_per_channel: usize,
}

impl Resampler {
    pub fn new(
        in_layout: &sys::AVChannelLayout,
        in_sample_fmt: sys::AVSampleFormat,
        in_sample_rate: i32,
        options: ResampleOptions,
    ) -> Result<Self> {
        options.validate()?;

        unsafe {
            let mut out_layout = mem::zeroed::<sys::AVChannelLayout>();
            sys::av_channel_layout_default(&raw mut out_layout, options.target_channels);

            let swr = SwrContext::new(
                &out_layout,
                options.target_sample_fmt,
                options.target_sample_rate,
                in_layout,
                in_sample_fmt,
                in_sample_rate,
            )?;

            sys::av_channel_layout_uninit(&raw mut out_layout);

            Ok(Self {
                swr,
                options,
                buffer: RawAudioBuffer::default(),
                actual_samples_per_channel: 0,
                stride_samples_per_channel: 0,
            })
        }
    }

    /// Returns the target sample format configured for this resampler.
    #[must_use]
    pub const fn target_sample_fmt(&self) -> sys::AVSampleFormat {
        self.options.target_sample_fmt
    }

    pub fn flush(&mut self) -> Result<()> {
        self.swr.flush()
    }

    /// Processes a single raw audio frame and writes the converted samples
    /// into the internal buffer.
    ///
    /// Passing `None` as the frame will flush any remaining buffered samples
    /// at the end of the stream.
    ///
    /// # Returns
    /// - `Ok(true)` if valid data was generated and is ready to be read.
    /// - `Ok(false)` if more input frames are needed to produce an output.
    /// - `Err` if a format mismatch or FFmpeg internal error occurs.
    pub fn process<T: AudioSample>(&mut self, frame: Option<&AudioFrame<'_>>) -> Result<bool> {
        let is_packed = self.options.target_sample_fmt == T::PACKED_FORMAT;
        let is_planar = self.options.target_sample_fmt == T::PLANAR_FORMAT;

        if !is_packed && !is_planar {
            return Err(AudioError::FormatMismatch);
        }

        unsafe {
            let raw_ptr = frame.map_or(ptr::null(), AudioFrame::as_ptr);

            let (in_data, in_samples) = if raw_ptr.is_null() {
                (ptr::null(), 0)
            } else {
                (
                    (*raw_ptr).extended_data as *const *const u8,
                    (*raw_ptr).nb_samples,
                )
            };

            debug_assert!(
                in_samples == 0 || !in_data.is_null(),
                "in_data is null but in_samples is > 0."
            );
            debug_assert!(in_samples >= 0, "in_samples cannot be negative.");

            let expected_out_samples = self.swr.get_out_samples(in_samples)?;
            if expected_out_samples <= 0 {
                self.actual_samples_per_channel = 0;
                return Ok(false);
            }

            let out_channels = self.options.target_channels as usize;

            let bytes_needed =
                (expected_out_samples as usize) * out_channels * std::mem::size_of::<T>();

            self.buffer.reserve_bytes(bytes_needed);
            self.stride_samples_per_channel = expected_out_samples as usize;

            let out_buf_slice = self.buffer.as_uninit_bytes_mut();

            debug_assert!(
                out_buf_slice.len() >= bytes_needed,
                "Rust slice length ({}) is smaller than the bytes promised to FFmpeg ({}). Buffer overflow imminent",
                out_buf_slice.len(),
                bytes_needed
            );

            let actual_samples = if is_planar {
                self.swr
                    .convert_planar(in_data, in_samples, out_buf_slice, expected_out_samples)?
            } else {
                self.swr
                    .convert_packed(in_data, in_samples, out_buf_slice)?
            };

            self.actual_samples_per_channel = actual_samples;
            Ok(self.actual_samples_per_channel > 0)
        }
    }

    /// Exposes the internally processed audio data as a typed slice.
    ///
    /// This method should only be called immediately after `process`
    /// returns `Ok(true)`. If there is no valid data, it returns an empty slice.
    #[must_use]
    pub const fn output_as<T: AudioSample>(&self) -> &[T] {
        if self.actual_samples_per_channel == 0 {
            return &[];
        }
        unsafe {
            self.buffer.as_typed_slice::<T>(
                self.actual_samples_per_channel * self.options.target_channels as usize,
            )
        }
    }

    /// Exposes the internally processed Planar audio data as a collection of slices.
    ///
    /// # Returns
    /// A `Vec` where each element is a slice representing one audio channel.
    #[must_use]
    pub fn output_planar_as<T: AudioSample>(&self) -> Vec<&[T]> {
        if self.actual_samples_per_channel == 0 {
            return vec![];
        }

        let channels_count = self.options.target_channels as usize;
        let mut channels = Vec::with_capacity(channels_count);

        let base_ptr = self.buffer.as_ptr::<T>();

        for ch in 0..channels_count {
            unsafe {
                let ch_start_ptr = base_ptr.add(ch * self.stride_samples_per_channel);

                let ch_slice =
                    std::slice::from_raw_parts(ch_start_ptr, self.actual_samples_per_channel);

                channels.push(ch_slice);
            }
        }

        channels
    }
}

/// A type-erased, low-level audio buffer designed for safe FFI interactions.
///
/// This buffer internally uses `Vec<MaybeUninit<f64>>` to guarantee strict
/// 8-byte memory alignment, which safely accommodates all standard FFmpeg
/// audio sample formats without triggering UB.
#[derive(Default)]
pub struct RawAudioBuffer {
    inner: Vec<MaybeUninit<f64>>,
}

impl RawAudioBuffer {
    /// Returns a raw pointer to the underlying memory, cast to `T`.
    ///
    /// This is useful for performing pointer arithmetic to skip over uninitialized
    /// memory gaps (e.g., in planar audio layouts) without triggering UB by creating
    /// a typed slice over uninitialized bytes.
    pub const fn as_ptr<T: AudioSample>(&self) -> *const T {
        self.inner.as_ptr().cast::<T>()
    }

    /// Reserves minimum physical capacity to hold the requested number of bytes.
    ///
    /// This method only increases the underlying `capacity` of the allocator.
    /// The `len` of the internal vector remains perpetually `0`. It calculates
    /// the required number of `f64` blocks to satisfy the byte requirement
    /// while maintaining the 8-byte alignment constraint.
    ///
    /// # Arguments
    /// * `required_bytes` - The absolute minimum number of bytes needed for
    ///   the upcoming FFI write operation.
    pub fn reserve_bytes(&mut self, required_bytes: usize) {
        let f64_count = required_bytes.div_ceil(mem::size_of::<f64>());
        self.inner.reserve(f64_count);
    }

    /// Exposes the entire allocated physical capacity as a mutable slice of
    /// uninitialized bytes.
    ///
    /// # Returns
    /// A mutable slice spanning the total reserved capacity, represented as
    /// `MaybeUninit<u8>`.
    pub const fn as_uninit_bytes_mut(&mut self) -> &mut [MaybeUninit<u8>] {
        let capacity_bytes = self.inner.capacity() * mem::size_of::<f64>();
        unsafe {
            std::slice::from_raw_parts_mut(
                self.inner.as_mut_ptr().cast::<MaybeUninit<u8>>(),
                capacity_bytes,
            )
        }
    }

    /// Casts the underlying memory and extracts a typed, initialized slice.
    ///
    /// # Safety
    /// This function performs unchecked type punning. The caller must guarantee
    /// all of the following:
    /// 1. **Initialization**: C side must have successfully written valid data
    ///    spanning at least `element_count` elements into the front of this buffer.
    /// 2. **Type Matching**: The physical bytes written by the FFI must exactly
    ///    match the memory layout and semantics of the requested Rust type `T`.
    /// 3. **Bounds**: `element_count * size_of::<T>()` must not exceed the
    ///    previously reserved capacity.
    pub const unsafe fn as_typed_slice<T: AudioSample>(&self, element_count: usize) -> &[T] {
        unsafe { std::slice::from_raw_parts(self.inner.as_ptr().cast::<T>(), element_count) }
    }
}
