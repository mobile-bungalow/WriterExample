mod conversion;
mod h264_encoder;
mod writer;

use godot::classes::{Engine, MovieWriter};
use godot::prelude::*;

use h264_encoder::H264Encoder;
pub use writer::FelliniWriter;

use ffmpeg_next as ffmpeg;

struct FelliniMovieWriter;

#[derive(Debug)]
enum Error {
    Setup,
    IoError,
    LimitReached,
    Validation(String),
    Encoding(String),
}

#[gdextension]
unsafe impl ExtensionLibrary for FelliniMovieWriter {
    fn min_level() -> InitLevel {
        InitLevel::Scene
    }

    fn on_level_init(level: InitLevel) {
        if level == InitLevel::Scene {
            godot_print!("Registering fellini writer singleton.");
            let writer = FelliniWriter::new_alloc();
            Engine::singleton()
                .register_singleton(StringName::from("Fellini"), writer.clone().upcast());
            MovieWriter::add_writer(writer.upcast());
        }
    }

    fn on_level_deinit(level: InitLevel) {
        if level == InitLevel::Scene {
            let mut engine = Engine::singleton();
            let singleton_name = StringName::from("Fellini");

            let singleton = engine
                .get_singleton(singleton_name.clone())
                .expect("cannot retrieve the singleton");

            engine.unregister_singleton(singleton_name);
            singleton.free();
        }
    }
}

enum Outputs {
    Color = 0,
    Depth = 1,
    MotionVectors = 2,
}

pub struct ConcreteEncoderSettings {
    // Frame rate in frames per second
    pub frame_rate: ffmpeg_next::Rational,
    // The conversion factor of units time per second
    // relevant for PTS calculations
    pub time_base: ffmpeg_next::Rational,
    // The format that incoming frames must be encoded in
    pub pixel_format: ffmpeg::format::Pixel,
    pub audio_sample_rate: u32,
}

#[derive(Debug, Default)]
pub struct EncoderSettings {
    // Frame rate in frames per second
    pub frame_rate: Option<ffmpeg_next::Rational>,
    // The conversion factor of units time per second
    // relevant for PTS calculations
    pub time_base: Option<ffmpeg_next::Rational>,
    // The format that incoming frames must be encoded in
    pub pixel_format: Option<ffmpeg::format::Pixel>,
    pub audio_sample_rate: Option<u32>,
}

impl ConcreteEncoderSettings {
    pub fn update(mut self, settings: EncoderSettings) -> Self {
        if let Some(frame_rate) = settings.frame_rate {
            self.frame_rate = frame_rate;
        }

        if let Some(time_base) = settings.time_base {
            self.time_base = time_base;
        }

        if let Some(pixel_format) = settings.pixel_format {
            self.pixel_format = pixel_format;
        }

        if let Some(audio_sample_rate) = settings.audio_sample_rate {
            self.audio_sample_rate = audio_sample_rate;
        }

        self
    }
}

pub(crate) trait Encoder: Sized {
    const CODEC: ffmpeg::codec::Id;
    const DEFAULT_SETTINGS: ConcreteEncoderSettings;
    const SUPPORTED_CONTAINERS: &'static [&'static str];

    fn new(
        width: u32,
        height: u32,
        path: std::path::PathBuf,
        settings: EncoderSettings,
    ) -> Result<Self, Error>;

    fn settings(&self) -> &ConcreteEncoderSettings;

    fn start(&mut self) -> Result<(), Error>;

    fn push_video_frame(&mut self, index: usize, frame: ffmpeg::frame::Video) -> Result<(), Error>;

    fn push_audio_frame(&mut self, index: usize, frame: ffmpeg::frame::Audio) -> Result<(), Error>;

    fn finish(&mut self) -> Result<(), Error>;
}

pub(crate) enum EncoderKind {
    H264(H264Encoder),
}

impl EncoderKind {
    pub fn codec(&self) -> ffmpeg::codec::Id {
        match self {
            EncoderKind::H264(_) => H264Encoder::CODEC,
        }
    }

    pub fn settings(&self) -> &ConcreteEncoderSettings {
        match self {
            EncoderKind::H264(h264) => h264.settings(),
        }
    }

    pub fn supported_containers(&self) -> &[&str] {
        match self {
            EncoderKind::H264(_) => H264Encoder::SUPPORTED_CONTAINERS,
        }
    }

    pub fn start(&mut self) -> Result<(), Error> {
        match self {
            EncoderKind::H264(h264) => h264.start(),
        }
    }

    pub fn finish(&mut self) -> Result<(), Error> {
        match self {
            EncoderKind::H264(h264) => h264.finish(),
        }
    }

    pub fn push_video_frame(
        &mut self,
        index: usize,
        frame: ffmpeg::frame::Video,
    ) -> Result<(), Error> {
        match self {
            EncoderKind::H264(h264) => h264.push_video_frame(index, frame),
        }
    }

    fn push_audio_frame(&mut self, index: usize, frame: ffmpeg::frame::Audio) -> Result<(), Error> {
        match self {
            EncoderKind::H264(h264) => h264.push_audio_frame(index, frame),
        }
    }
}
