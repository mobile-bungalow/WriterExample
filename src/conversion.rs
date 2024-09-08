use ffmpeg_next::format::Pixel;
use godot::classes::image::Format;

pub enum ConversionError {
    Unsupported(String),
}

fn gd_to_ffmpeg_fmt(value: Format) -> Result<Pixel, ConversionError> {
    match value {
        Format::RGBA8 => Ok(Pixel::RGBA),
        e => Err(ConversionError::Unsupported(format!("{e:?}"))),
    }
}
