# ffmpeg-audio

Minimal FFmpeg audio decoding wrapper. Network handled by the caller via `Read + Seek`.

## Features

- `AudioReader::new(impl Read + Seek)` for raw decoding, with an optional, flexible resampling pipeline.
- FFmpeg vendored, no env vars.
- Decoders: MP3, AAC, FLAC, Opus, Vorbis, ALAC, APE, WAV, WMA, DSD, DCA, EAC3, TrueHD.

## Usage

```toml
[dependencies]
ffmpeg_audio = { git = "https://github.com/apoint123/ffmpeg-audio" }

```

```rust
use std::fs::File;
use ffmpeg_audio::{AudioReader, ResampleOptions};

// 1. Initialize the pure decoding engine
let reader = AudioReader::new(File::open("song.mp3")?)?;

// 2. Configure target audio parameters (e.g., 48kHz, Stereo, 32-bit Float)
let options = ResampleOptions::new()
    .sample_rate(48000)
    .channels(2)
    .format::<f32>();

// 3. Transform into a resampled data pipeline
let mut resampled = reader.into_resampled(options)?;

// 4. Safely pull typed audio frames
while let Some(samples) = resampled.receive_frame_as::<f32>()? {
    // `samples` is strictly typed as &[f32]
    // process your interleaved samples here
}

```

## HTTP streams

Implement `Read + Seek` over HTTP Range and pass it in:

```rust
struct HttpRangeSource { /* ureq agent + cursor */ }
impl Read for HttpRangeSource { /* ... */ }
impl Seek for HttpRangeSource { /* ... */ }

// Decode and resample seamlessly from an HTTP source
let source = HttpRangeSource::new(url)?;
let reader = AudioReader::new(source)?;
let options = ResampleOptions::new().sample_rate(48000).channels(2).format::<i16>();

let mut resampled = reader.into_resampled(options)?;

while let Some(samples) = resampled.receive_frame_as::<i16>()? {
    // Process stream data chunks
}

```

## Not in scope

Video, encoders, muxers, filters, swscale, hardware acceleration, devices, network protocols.
