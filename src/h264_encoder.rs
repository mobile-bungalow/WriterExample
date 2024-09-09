use crate::{ConcreteEncoderSettings, Encoder, EncoderSettings, Error};

use ffmpeg_next::{self as ffmpeg, *};

pub struct H264Encoder {
    width: u32,
    height: u32,
    settings: ConcreteEncoderSettings,
    output_context: format::context::Output,
    encoder: Option<encoder::Video>,
}

const STREAM_INDEX: usize = 0;

impl Encoder for H264Encoder {
    const CODEC: ffmpeg::codec::Id = ffmpeg::codec::Id::H264;

    const DEFAULT_SETTINGS: ConcreteEncoderSettings = ConcreteEncoderSettings {
        frame_rate: ffmpeg_next::Rational(24, 1),
        time_base: ffmpeg_next::Rational(1, 90_000),
        pixel_format: ffmpeg::format::Pixel::YUV420P,
        audio_sample_rate: 44_100,
    };

    const SUPPORTED_CONTAINERS: &'static [&'static str] = &["mp4", "mov", "avi"];

    fn new(
        width: u32,
        height: u32,
        path: std::path::PathBuf,
        settings: EncoderSettings,
    ) -> Result<Self, Error> {
        let settings = Self::DEFAULT_SETTINGS.update(settings);

        let output_context = format::output(&path).map_err(|_| Error::Setup)?;

        Ok(Self {
            width,
            height,
            settings,
            output_context,
            encoder: None,
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

        let codec = encoder::find(Self::CODEC)
            .ok_or_else(|| Error::Encoding("Codec not found".to_string()))?;

        let mut video_stream = self
            .output_context
            .add_stream(codec)
            .map_err(|e| Error::Encoding(format!("Could not add video stream: {}", e)))?;

        let mut encoder = codec::context::Context::new_with_codec(codec)
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
        }

        let mut dict = Dictionary::new();
        dict.set("preset", "medium");

        let encoder = encoder
            .open_with(dict)
            .map_err(|e| Error::Encoding(format!("Could not open encoder: {}", e)))?;

        video_stream.set_parameters(&encoder);

        self.encoder = Some(encoder);

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
            .encoder
            .as_mut()
            .ok_or_else(|| Error::Encoding("Encoder not initialized".to_string()))?;

        encoder.send_frame(&frame).map_err(|e| {
            Error::Encoding(format!("Could not send audio frame to encoder: {}", e))
        })?;

        self.flush()
    }

    fn push_video_frame(
        &mut self,
        index: usize,
        mut frame: ffmpeg::frame::Video,
    ) -> Result<(), Error> {
        let pts = crate::conversion::frame_to_pts(
            index as i64,
            self.settings.frame_rate.0.into(),
            self.settings.time_base.1.into(),
        );

        frame.set_kind(picture::Type::None);
        frame.set_pts(Some(pts));

        let encoder = self
            .encoder
            .as_mut()
            .ok_or_else(|| Error::Encoding("Encoder not initialized".to_string()))?;

        encoder
            .send_frame(&frame)
            .map_err(|e| Error::Encoding(format!("Could not send frame to encoder: {}", e)))?;

        self.flush()
    }

    fn finish(&mut self) -> Result<(), Error> {
        if let Some(ref mut encoder) = self.encoder {
            encoder
                .send_eof()
                .map_err(|e| Error::Encoding(format!("Could not send EOF to encoder: {}", e)))?;

            self.flush()?;
        }

        self.output_context
            .write_trailer()
            .map_err(|e| Error::Encoding(format!("Could not write trailer: {}", e)))?;

        Ok(())
    }
}

impl H264Encoder {
    pub fn flush(&mut self) -> Result<(), Error> {
        let encoder = self
            .encoder
            .as_mut()
            .ok_or_else(|| Error::Encoding("Encoder not initialized".to_string()))?;

        let mut packet = Packet::empty();

        while encoder.receive_packet(&mut packet).is_ok() {
            packet.set_stream(STREAM_INDEX);

            packet.rescale_ts(
                self.settings.time_base,
                self.output_context
                    .stream(STREAM_INDEX)
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
