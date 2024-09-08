use ffmpeg_next::format::Pixel;
use godot::classes::image::Format;

pub enum ConversionError {
    Unsupported(String),
}

/// Converts frame index to a presentation time stamp
pub fn frame_to_pts(frame_idx: i64, fps: i64, ticks_per_second: i64) -> i64 {
    let seconds = frame_idx as f64 / fps as f64;
    (seconds * ticks_per_second as f64).round() as i64
}

pub fn gd_to_ffmpeg_fmt(value: Format) -> Result<Pixel, ConversionError> {
    match value {
        Format::RGBA8 => Ok(Pixel::RGBA),
        e => Err(ConversionError::Unsupported(format!("{e:?}"))),
    }
}
