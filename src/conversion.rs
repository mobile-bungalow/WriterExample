use ffmpeg_next::channel_layout::ChannelLayout;
use ffmpeg_next::format::Pixel;
use godot::classes::audio_server::SpeakerMode;
use godot::classes::image::Format;

use godot::classes::{DisplayServer, Image, RenderingServer};
use godot::prelude::*;

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

pub fn get_yuva420p_image() -> Option<Gd<Image>> {
    let rs = RenderingServer::singleton();
    let ds = DisplayServer::singleton();

    let main_window_id = DisplayServer::MAIN_WINDOW_ID;

    let main_vp_rid = todo!();

    let main_vp_texture = rs.viewport_get_texture(main_vp_rid);
    let mut vp_tex = rs.texture_2d_get(main_vp_texture);

    vp_tex

    //if rs.viewport_is_using_hdr_2d(main_vp_rid) {
    //    vp_tex.convert(Image::FORMAT_RGBA8);
    //    vp_tex.linear_to_srgb();
    //}

    // RID main_vp_rid = RenderingServer::get_singleton()->viewport_find_from_screen_attachment(DisplayServer::MAIN_WINDOW_ID);
    // RID main_vp_texture = RenderingServer::get_singleton()->viewport_get_texture(main_vp_rid);
    // Ref<Image> vp_tex = RenderingServer::get_singleton()->texture_2d_get(main_vp_texture);
    // if (RenderingServer::get_singleton()->viewport_is_using_hdr_2d(main_vp_rid)) {
    // 	vp_tex->convert(Image::FORMAT_RGBA8);
    // 	vp_tex->linear_to_srgb();
    // }
    //
    // return vp_text;
}
