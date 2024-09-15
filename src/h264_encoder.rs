use crate::{
    conversion::ConversionContext, ConcreteEncoderSettings, Encoder, EncoderSettings, Error,
};
use ffi::EAGAIN;
use godot::classes::Image;
use godot::prelude::Gd;

use ffmpeg_next::{self as ffmpeg, *};

pub struct H264Encoder {
    width: u32,
    height: u32,
    converter: crate::conversion::ConversionContext,
    settings: ConcreteEncoderSettings,
    output_context: format::context::Output,
    video_encoder: Option<encoder::Video>,
    audio_encoder: Option<encoder::Audio>,
}

const STREAM_INDEX: usize = 0;

impl Encoder for H264Encoder {
    const VIDEO_CODEC: ffmpeg::codec::Id = ffmpeg::codec::Id::H264;
    const AUDIO_CODEC: ffmpeg::codec::Id = ffmpeg::codec::Id::FLAC;

    const DEFAULT_SETTINGS: ConcreteEncoderSettings = ConcreteEncoderSettings {
        frame_rate: ffmpeg_next::Rational(24, 1),
        time_base: ffmpeg_next::Rational(1, 90_000),
        pixel_format: ffmpeg::format::Pixel::YUV420P,
        audio_sample_rate: 44_100,
    };

    const SUPPORTED_CONTAINERS: &'static [&'static str] = &["mp4"];

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
        audio_encoder.set_time_base(self.settings.time_base);
        audio_encoder.set_rate(Self::DEFAULT_SETTINGS.audio_sample_rate as i32);
        audio_encoder.set_max_bit_rate(320_000);

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

        if global_header {
            encoder.set_flags(codec::Flags::GLOBAL_HEADER);
            audio_encoder.set_flags(codec::Flags::GLOBAL_HEADER);
        }

        let mut dict = Dictionary::new();
        dict.set("preset", "medium");

        let encoder = encoder
            .open_with(dict)
            .map_err(|e| Error::Encoding(format!("Could not open video encoder: {}", e)))?;

        let audio_encoder = audio_encoder
            .open()
            .map_err(|e| Error::Encoding(format!("Could not open audio encoder: {}", e)))?;

        let mut video_stream = self
            .output_context
            .add_stream(video_codec)
            .map_err(|e| Error::Encoding(format!("Could not add video stream: {}", e)))?;

        video_stream.set_parameters(&encoder);

        let mut audio_stream = self
            .output_context
            .add_stream(audio_codec)
            .map_err(|e| Error::Encoding(format!("Could not add audio stream: {}", e)))?;

        audio_stream.set_parameters(&audio_encoder);

        self.video_encoder = Some(encoder);
        self.audio_encoder = Some(audio_encoder);

        self.output_context
            .write_header()
            .map_err(|e| Error::Encoding(format!("Could not write header: {}", e)))?;

        Ok(())
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

        let encoder = self
            .audio_encoder
            .as_mut()
            .ok_or_else(|| Error::Encoding("audio Encoder not initialized".to_string()))?;

        match encoder.send_frame(&frame) {
            Ok(_) => self.flush(1),
            Err(ffmpeg::Error::Other { errno }) if errno == EAGAIN => return Ok(()),
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
        self.converter.convert(frame_image, &mut frame);

        frame.set_kind(picture::Type::None);
        frame.set_pts(Some(pts));

        let encoder = self
            .video_encoder
            .as_mut()
            .ok_or_else(|| Error::Encoding("Encoder not initialized".to_string()))?;

        encoder
            .send_frame(&frame)
            .map_err(|e| Error::Encoding(format!("Could not send frame to encoder: {}", e)))?;

        self.flush(0)
    }

    fn finish(&mut self) -> Result<(), Error> {
        if let Some(ref mut encoder) = self.video_encoder {
            encoder
                .send_eof()
                .map_err(|e| Error::Encoding(format!("Could not send EOF to encoder: {}", e)))?;

            self.flush(0)?;
            self.flush(1)?;
        }

        self.output_context
            .write_trailer()
            .map_err(|e| Error::Encoding(format!("Could not write trailer: {}", e)))?;

        Ok(())
    }
}

impl H264Encoder {
    pub fn flush(&mut self, audio: usize) -> Result<(), Error> {
        let encoder = self
            .video_encoder
            .as_mut()
            .ok_or_else(|| Error::Encoding("Encoder not initialized".to_string()))?;

        let mut packet = Packet::empty();

        while encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(STREAM_INDEX + audio);

            packet.rescale_ts(
                self.settings.time_base,
                self.output_context
                    .stream(STREAM_INDEX + audio)
                    .unwrap()
                    .time_base(),
            );

            packet
                .write_interleaved(&mut self.output_context)
                .map_err(|e| Error::Encoding(format!("Could not write packet: {}", e)))?;
        }

        Ok(())
    }
}
