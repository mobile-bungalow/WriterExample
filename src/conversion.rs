use ffmpeg_next::channel_layout::ChannelLayout;
use ffmpeg_next::format::Pixel;
use godot::classes::image::Format;
use godot::engine::audio_server::SpeakerMode;

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

// Audio blocks are a u32 c-array.
pub fn audio_block_size_per_frame(
    channel_layout: SpeakerMode,
    mix_rate: u32,
    frame_rate: u32,
) -> u32 {
    const BIT_DEPTH: u32 = 32;

    let channel_ct = match channel_layout {
        SpeakerMode::STEREO => 2,
        SpeakerMode::SURROUND_31 => 4,
        SpeakerMode::SURROUND_51 => 6,
        SpeakerMode::SURROUND_71 => 8,
        _ => 2,
    };

    // One u32 aligned item
    let block_alignment = (BIT_DEPTH / 8) * channel_ct;

    (mix_rate / frame_rate) * block_alignment
}

pub fn godot_speaker_mode_to_ffmpeg(channel_layout: SpeakerMode) -> ChannelLayout {
    match channel_layout {
        SpeakerMode::STEREO => ChannelLayout::STEREO,
        SpeakerMode::SURROUND_31 => ChannelLayout::QUAD,
        SpeakerMode::SURROUND_51 => ChannelLayout::HEXAGONAL,
        SpeakerMode::SURROUND_71 => ChannelLayout::OCTAGONAL,
        _ => ChannelLayout::MONO,
    }
}
