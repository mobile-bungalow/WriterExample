use crate::{
    conversion::ConversionContext, ConcreteEncoderSettings, Encoder, EncoderSettings, Error,
};
use error::EAGAIN;
use godot::classes::Image;
use godot::prelude::Gd;

use ffmpeg_next::{self as ffmpeg, *};

pub struct Av1Encoder {
    width: u32,
    height: u32,
    converter: crate::conversion::ConversionContext,
    settings: ConcreteEncoderSettings,
    output_context: format::context::Output,
    video_encoder: Option<encoder::Video>,
    audio_encoder: Option<encoder::Audio>,
}

const VIDEO_STREAM: usize = 0;
const AUDIO_STREAM: usize = 1;
const MAX_AUDIO_BITRATE: usize = 128_000;
const MAX_VIDEO_BITRATE: usize = 200_000;

impl Encoder for Av1Encoder {
    const VIDEO_CODEC: ffmpeg::codec::Id = ffmpeg::codec::Id::AV1;
    const AUDIO_CODEC: ffmpeg::codec::Id = ffmpeg::codec::Id::FLAC;

    const DEFAULT_SETTINGS: ConcreteEncoderSettings = ConcreteEncoderSettings {
        frame_rate: ffmpeg_next::Rational(24, 1),
        time_base: ffmpeg_next::Rational(1, 90_000),
        pixel_format: ffmpeg::format::Pixel::YUV420P,
        audio_sample_rate: 44_100,
        audio_enabled: false,
    };

    const SUPPORTED_CONTAINERS: &'static [&'static str] = &["mkv", "mp4", "webm"];

    fn new(
        width: u32,
        height: u32,
        path: std::path::PathBuf,
        settings: EncoderSettings,
    ) -> Result<Self, Error> {
        let settings = Self::DEFAULT_SETTINGS.update(settings);

        let output_context = format::output(&path).map_err(|_| Error::Setup)?;

        let converter = ConversionContext::new(
            godot::classes::image::Format::RGBA8,
            settings.pixel_format,
            width,
            height,
        )?;

        Ok(Self {
            converter,
            width,
            height,
            settings,
            output_context,
            video_encoder: None,
            audio_encoder: None,
        })
    }

    fn settings<'a>(&'a self) -> &'a ConcreteEncoderSettings {
        &self.settings
    }

    fn start(&mut self) -> Result<(), Error> {
        let global_header = self
            .output_context
            .format()
            .flags()
            .contains(format::Flags::GLOBAL_HEADER);

        let audio_codec = encoder::find(Self::AUDIO_CODEC)
            .ok_or_else(|| Error::Encoding("Audio Codec not found".to_string()))?;

        let mut audio_enc = if self.settings().audio_enabled {
            let mut audio_encoder = codec::context::Context::new_with_codec(audio_codec)
                .encoder()
                .audio()
                .map_err(|e| {
                    Error::Encoding(format!("Could not create audio encoder context: {}", e))
                })?;

            audio_encoder.set_channel_layout(ChannelLayout::STEREO);
            audio_encoder.set_format(format::Sample::I32(format::sample::Type::Packed));
            audio_encoder.set_frame_rate(Some((
                Self::DEFAULT_SETTINGS.audio_sample_rate as i32,
                1i32,
            )));
            audio_encoder.set_compression(Some(4));
            audio_encoder.set_time_base(self.settings.time_base);
            audio_encoder.set_rate(Self::DEFAULT_SETTINGS.audio_sample_rate as i32);
            audio_encoder.set_max_bit_rate(MAX_AUDIO_BITRATE);

            Some(audio_encoder)
        } else {
            None
        };

        let video_codec = encoder::find(Self::VIDEO_CODEC)
            .ok_or_else(|| Error::Encoding("Video Codec not found".to_string()))?;

        let mut encoder = codec::context::Context::new_with_codec(video_codec)
            .encoder()
            .video()
            .map_err(|e| Error::Encoding(format!("Could not create encoder context: {}", e)))?;

        encoder.set_height(self.height);
        encoder.set_width(self.width);
        encoder.set_time_base(self.settings.time_base);
        encoder.set_format(self.settings.pixel_format);
        encoder.set_frame_rate(Some(self.settings.frame_rate));
        encoder.set_bit_rate(MAX_VIDEO_BITRATE);
        encoder.set_threading(ffmpeg::threading::Config {
            kind: threading::Type::Frame,
            count: 32,
        });

        if global_header {
            encoder.set_flags(codec::Flags::GLOBAL_HEADER);
            if let Some(ref mut audio_encoder) = audio_enc {
                audio_encoder.set_flags(codec::Flags::GLOBAL_HEADER);
            }
        }

        let mut dict = Dictionary::new();
        dict.set("crf", "23");
        dict.set("cpu-used", "8");
        dict.set("tiles", "4x4");
        dict.set("row-mt", "1");
        dict.set("enable-keyframe-filtering", "0");

        let encoder = encoder
            .open_with(dict)
            .map_err(|e| Error::Encoding(format!("Could not open video encoder: {}", e)))?;

        if let Some(audio_encoder) = audio_enc {
            let audio_encoder = audio_encoder
                .open()
                .map_err(|e| Error::Encoding(format!("Could not open audio encoder: {}", e)))?;

            let mut audio_stream = self
                .output_context
                .add_stream(audio_codec)
                .map_err(|e| Error::Encoding(format!("Could not add audio stream: {}", e)))?;

            audio_stream.set_parameters(&audio_encoder);
            self.audio_encoder = Some(audio_encoder);
        }

        let mut video_stream = self
            .output_context
            .add_stream(video_codec)
            .map_err(|e| Error::Encoding(format!("Could not add video stream: {}", e)))?;

        video_stream.set_parameters(&encoder);

        self.video_encoder = Some(encoder);

        self.output_context
            .write_header()
            .map_err(|e| Error::Encoding(format!("Could not write header: {}", e)))?;

        Ok(())
    }

    fn audio_frame_size(&self) -> u32 {
        self.audio_encoder.as_ref().map_or(1024, |enc| {
            if enc.0.codec().map_or(false, |codec| {
                codec
                    .capabilities()
                    .contains(ffmpeg::codec::capabilities::Capabilities::VARIABLE_FRAME_SIZE)
            }) {
                1024
            } else {
                enc.frame_size()
            }
        })
    }

    fn push_audio_frame(
        &mut self,
        index: usize,
        mut frame: ffmpeg::frame::Audio,
    ) -> Result<(), Error> {
        let pts = crate::conversion::frame_to_pts(
            index as i64,
            self.settings.frame_rate.0.into(),
            self.settings.time_base.1.into(),
        );

        frame.set_pts(Some(pts));

        let mut err = Err(ffmpeg_next::Error::Other { errno: EAGAIN });

        while let Err(ffmpeg_next::Error::Other { errno: EAGAIN }) = err {
            let encoder = self
                .audio_encoder
                .as_mut()
                .ok_or_else(|| Error::Encoding("audio Encoder not initialized".to_string()))?;
            err = encoder.send_frame(&frame);
            self.flush(AUDIO_STREAM)?;
        }

        match err {
            Ok(_) => Ok(()),
            Err(e) => {
                return Err(Error::Encoding(format!(
                    "Could not send audio frame to encoder: {}",
                    e
                )));
            }
        }
    }

    fn push_video_frame(&mut self, index: usize, frame_image: Gd<Image>) -> Result<(), Error> {
        let pts = crate::conversion::frame_to_pts(
            index as i64,
            self.settings.frame_rate.0.into(),
            self.settings.time_base.1.into(),
        );

        let width = self.converter.width as u32;
        let height = self.converter.height as u32;

        let mut frame = frame::Video::new(self.settings().pixel_format, width, height);
        frame.set_kind(picture::Type::None);
        frame.set_pts(Some(pts));

        self.converter.convert(frame_image, &mut frame);

        let mut err = Err(ffmpeg_next::Error::Other { errno: EAGAIN });

        let encoder = self
            .video_encoder
            .as_mut()
            .ok_or_else(|| Error::Encoding("audio Encoder not initialized".to_string()))?;

        while let Err(ffmpeg_next::Error::Other { errno: EAGAIN }) = err {
            err = encoder.send_frame(&frame);
        }

        if err.is_ok() {
            self.flush(VIDEO_STREAM)
        } else {
            Err(Error::Encoding(format!(
                "Packet could not be sent to encoder {err:?}"
            )))
        }
    }

    fn finish(&mut self) -> Result<(), Error> {
        if let Some(ref mut encoder) = self.video_encoder {
            encoder
                .send_eof()
                .map_err(|e| Error::Encoding(format!("Could not send EOF to encoder: {}", e)))?;

            self.flush(VIDEO_STREAM)?;

            if self.settings().audio_enabled {
                self.flush(AUDIO_STREAM)?;
            }
        }

        self.output_context
            .write_trailer()
            .map_err(|e| Error::Encoding(format!("Could not write trailer: {}", e)))?;

        Ok(())
    }
}

impl Av1Encoder {
    pub fn flush(&mut self, stream: usize) -> Result<(), Error> {
        if stream == AUDIO_STREAM {
            let encoder = self
                .audio_encoder
                .as_mut()
                .ok_or_else(|| Error::Encoding("Encoder not initialized".to_string()))?;

            let mut packet = Packet::empty();

            while encoder.receive_packet(&mut packet).is_ok() {
                packet.set_stream(AUDIO_STREAM);

                packet.rescale_ts(
                    self.settings.time_base,
                    self.output_context
                        .stream(AUDIO_STREAM)
                        .unwrap()
                        .time_base(),
                );

                packet
                    .write_interleaved(&mut self.output_context)
                    .map_err(|e| Error::Encoding(format!("Could not write packet: {}", e)))?;
            }
        } else {
            let encoder = self
                .video_encoder
                .as_mut()
                .ok_or_else(|| Error::Encoding("Encoder not initialized".to_string()))?;

            let mut packet = Packet::empty();

            while let Ok(_) = encoder.receive_packet(&mut packet) {
                packet.set_stream(VIDEO_STREAM);

                packet.rescale_ts(
                    self.settings.time_base,
                    self.output_context
                        .stream(VIDEO_STREAM)
                        .unwrap()
                        .time_base(),
                );

                packet
                    .write_interleaved(&mut self.output_context)
                    .map_err(|e| Error::Encoding(format!("Could not write packet: {}", e)))?;
            }
        }

        Ok(())
    }
}
