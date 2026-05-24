use super::averror;

#[must_use]
pub const fn fferr(tag: &[u8; 4]) -> i32 {
    -u32::from_le_bytes(*tag).cast_signed()
}

#[must_use]
pub const fn fferr_f8(tag: &[u8; 3]) -> i32 {
    let bytes = [0xF8, tag[0], tag[1], tag[2]];
    -u32::from_le_bytes(bytes).cast_signed()
}

/// End of file
pub const AVERROR_EOF: i32 = fferr(b"EOF ");

/// Invalid data found when processing input
pub const AVERROR_INVALIDDATA: i32 = fferr(b"INDA");

/// Resource temporarily unavailable
pub const AVERROR_EAGAIN: i32 = averror(libc::EAGAIN);

/// Not enough space
pub const AVERROR_ENOMEM: i32 = averror(libc::ENOMEM);

/// Decoder not found
pub const AVERROR_DECODER_NOT_FOUND: i32 = fferr_f8(b"DEC");

/// Passing this as the "whence" parameter to a seek function causes it to
/// return the filesize without seeking anywhere.
///
/// Supporting this is optional.
/// If it is not supported then the seek function will return <0.
pub const AVSEEK_SIZE: i32 = 0x10000;

/// OR'ing this flag into the "whence" parameter enables force-seeking.
///
/// It may seek by any means (like reopening and linear reading) or other
/// normally unreasonable means that can be extremely slow.
/// This is the default and therefore ignored by the seek code since 2010.
pub const AVSEEK_FORCE: i32 = 0x20000;
