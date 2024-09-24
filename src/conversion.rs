use ffmpeg_next::channel_layout::ChannelLayout;
use ffmpeg_next::format::Pixel;
use godot::classes::audio_server::SpeakerMode;
use godot::classes::image::Format;
use godot::engine::rendering_device::{
    DataFormat, SamplerFilter, ShaderStage, TextureUsageBits, UniformType,
};

use crate::Error;
use godot::classes::{DisplayServer, Image, RenderingServer};
use godot::engine::{RdSamplerState, RdUniform, RenderingDevice};
use godot::prelude::*;

/// Converts frame index to a presentation time stamp
pub fn frame_to_pts(frame_idx: i64, fps: i64, ticks_per_second: i64) -> i64 {
    let seconds = frame_idx as f64 / fps as f64;
    (seconds * ticks_per_second as f64).round() as i64
}

// Audio blocks are a i32 c-array.
pub fn audio_array_size(channel_layout: SpeakerMode, sample_ct: u32) -> usize {
    let channel_ct = match channel_layout {
        SpeakerMode::STEREO => 2usize,
        SpeakerMode::SURROUND_31 => 4,
        SpeakerMode::SURROUND_51 => 6,
        SpeakerMode::SURROUND_71 => 8,
        _ => 2,
    };

    sample_ct as usize * std::mem::size_of::<i32>() * channel_ct
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

/// Texture containers
/// The scratch buffer is used to store the rgb frame to be yuv'd
/// Note that this is really inefficient, as currently the
/// engine copies to cpu, and we upload back.
pub enum Channels {
    YUVA420p {
        scratch: Rid,
        y: Rid,
        u: Rid,
        v: Rid,
        a: Rid,
    },
}

pub struct ConversionContext {
    channels: Channels,
    sampler: Rid,
    pub width: u32,
    pub height: u32,
    shader: Rid,
    device: Gd<RenderingDevice>,
    uniforms: Rid,
    pipeline: Rid,
}

impl ConversionContext {
    pub fn new(from: Format, to: Pixel, width: u32, height: u32) -> Result<Self, crate::Error> {
        let render_server = RenderingServer::singleton();

        let mut rd = render_server
            .create_local_rendering_device()
            .ok_or(Error::ConversionError("No render device".into()))?;

        let mut src = godot::classes::RdShaderSource::new_gd();

        src.set_stage_source(
            ShaderStage::COMPUTE,
            include_str!("./glsl/rgb_to_yuv420p.glsl").into(),
        );

        let spirv = rd
            .shader_compile_spirv_from_source(src)
            .ok_or(Error::ConversionError("failed to compile source".into()))?;

        let shader = rd.shader_create_from_spirv(spirv);

        let pipeline = rd.compute_pipeline_create(shader);

        let data_tex_alloc = |rd: &mut Gd<RenderingDevice>, w, h, bind| {
            let mut view = godot::classes::RdTextureView::new_gd();
            view.set_format_override(DataFormat::R8_UNORM);

            let mut fmt = godot::classes::RdTextureFormat::new_gd();
            fmt.set_format(DataFormat::R8_UNORM);
            fmt.set_usage_bits(TextureUsageBits::STORAGE_BIT | TextureUsageBits::CAN_COPY_FROM_BIT);
            fmt.set_width(w);
            fmt.set_height(h);

            let tex = rd.texture_create(fmt, view);

            let mut tex_uniform = RdUniform::new_gd();
            tex_uniform.set_binding(bind);
            tex_uniform.set_uniform_type(UniformType::IMAGE);
            tex_uniform.add_id(tex.clone());
            (tex_uniform, tex)
        };

        let scratch_tex_alloc = |rd: &mut Gd<RenderingDevice>, w, h, bind| {
            let default_view = godot::classes::RdTextureView::new_gd();

            let mut fmt = godot::classes::RdTextureFormat::new_gd();
            fmt.set_format(DataFormat::R8G8B8A8_UNORM);
            fmt.set_usage_bits(
                TextureUsageBits::CAN_UPDATE_BIT
                    | TextureUsageBits::SAMPLING_BIT
                    | TextureUsageBits::CAN_COPY_FROM_BIT,
            );
            fmt.set_width(w);
            fmt.set_height(h);

            let scratch_tex = rd.texture_create(fmt, default_view);

            let mut tex_uniform = RdUniform::new_gd();
            tex_uniform.set_binding(bind);
            tex_uniform.set_uniform_type(UniformType::TEXTURE);
            tex_uniform.add_id(scratch_tex.clone());
            (tex_uniform, scratch_tex)
        };

        let mut state = RdSamplerState::new_gd();
        state.set_min_filter(SamplerFilter::NEAREST);
        state.set_mag_filter(SamplerFilter::NEAREST);

        let sampler = rd.sampler_create(state);
        let mut sampler_uni = RdUniform::new_gd();
        sampler_uni.add_id(sampler);
        sampler_uni.set_uniform_type(UniformType::SAMPLER);
        sampler_uni.set_binding(5);

        let (uniforms, channels) = match to {
            Pixel::YUVA420P | Pixel::YUV420P => {
                let (scratch_uni, scratch) = scratch_tex_alloc(&mut rd, width, height, 0);
                let (y_uni, y) = data_tex_alloc(&mut rd, width, height, 1);
                let (u_uni, u) = data_tex_alloc(&mut rd, width / 2, height / 2, 2);
                let (v_uni, v) = data_tex_alloc(&mut rd, width / 2, height / 2, 3);
                let (a_uni, a) = data_tex_alloc(&mut rd, width, height, 4);

                let uniforms = Array::from(&[
                    scratch_uni.clone(),
                    y_uni.clone(),
                    u_uni.clone(),
                    v_uni.clone(),
                    a_uni.clone(),
                    sampler_uni.clone(),
                ]);
                let uniforms = rd.uniform_set_create(uniforms.into(), shader, 0);
                (
                    uniforms,
                    Channels::YUVA420p {
                        scratch,
                        y,
                        u,
                        v,
                        a,
                    },
                )
            }
            _ => {
                return Err(crate::Error::ConversionError(format!(
                    "Unsupported Conversion {from:?} : {to:?}"
                )))
            }
        };

        Ok(Self {
            sampler,
            uniforms,
            device: rd,
            width,
            height,
            channels,
            shader,
            pipeline,
        })
    }

    pub fn convert(
        &mut self,
        mut input_image: Gd<Image>,
        frame: &mut ffmpeg_next::util::frame::Video,
    ) {
        input_image.resize(self.width as i32, self.height as i32);
        input_image.convert(Format::RGBA8);

        match self.channels {
            Channels::YUVA420p { scratch, .. } => {
                self.device
                    .texture_update(scratch, 0, input_image.get_data());
            }
        }

        let compute_list = self.device.compute_list_begin();

        self.device
            .compute_list_bind_compute_pipeline(compute_list, self.pipeline);

        self.device
            .compute_list_bind_uniform_set(compute_list, self.uniforms, 0);

        self.device.compute_list_dispatch(
            compute_list,
            (self.width + 15) as u32 / 16,
            (self.width + 15) as u32 / 16,
            1,
        );
        self.device.compute_list_end();

        self.device.submit();
        self.device.sync();

        for plane in 0..frame.planes() {
            let buf = frame.data_mut(plane);
            match plane {
                0 => {
                    // luminance
                    match self.channels {
                        Channels::YUVA420p { y, .. } => {
                            let tex = self.device.texture_get_data(y, 0);
                            //dbg!(&tex.as_slice());
                            buf.copy_from_slice(tex.as_slice());
                        }
                    }
                }
                1 => match self.channels {
                    Channels::YUVA420p { u, .. } => {
                        let tex = self.device.texture_get_data(u, 0);
                        buf.copy_from_slice(tex.as_slice());
                    }
                },
                2 => match self.channels {
                    Channels::YUVA420p { v, .. } => {
                        let tex = self.device.texture_get_data(v, 0);
                        buf.copy_from_slice(tex.as_slice());
                    }
                },
                3 => match self.channels {
                    Channels::YUVA420p { a, .. } => {
                        let tex = self.device.texture_get_data(a, 0);
                        buf.copy_from_slice(tex.as_slice());
                    }
                    _ => panic!("no alpha channel"),
                },
                _ => panic!("unsupported plane count"),
            }
        }
    }
}

impl Drop for ConversionContext {
    fn drop(&mut self) {
        match &mut self.channels {
            Channels::YUVA420p {
                scratch,
                y,
                u,
                v,
                a,
                ..
            } => {
                self.device.free_rid(*y);
                self.device.free_rid(*u);
                self.device.free_rid(*v);
                self.device.free_rid(*a);
                self.device.free_rid(*scratch);
            }
        };

        self.device.free_rid(self.uniforms);
        self.device.free_rid(self.pipeline);
        self.device.free_rid(self.sampler);
        self.device.free_rid(self.shader);
    }
}
