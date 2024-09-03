use ffmpeg::{
    codec, decoder, encoder, format, frame, log, media, picture, Dictionary, Packet, Rational,
};
use ffmpeg_next as ffmpeg;

use godot::engine::audio_server::SpeakerMode;
use godot::engine::MovieWriter;
use godot::global::Error as GdError;
use godot::prelude::*;
use std::ffi::c_void;
use std::path::PathBuf;

#[derive(GodotClass)]
#[class(base=MovieWriter)]
pub struct FelliniWriter {
    base: Base<MovieWriter>,
    settings: Settings,
    state: FelliniState,
}

pub struct Settings {
    pub codec: ffmpeg::codec::Id,
    pub max_file_size: usize,
    pub audio_mix_rate: u32,
}

impl Settings {
    const DEFAULT_MAX_FILE_SIZE: usize = 1 << 30;
    const DEFAULT_MIX_RATE_HZ: u32 = 44_100;
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            codec: ffmpeg::codec::Id::H264,
            max_file_size: Self::DEFAULT_MAX_FILE_SIZE,
            audio_mix_rate: Self::DEFAULT_MIX_RATE_HZ,
        }
    }
}

enum FelliniState {
    PreRecording,
    Recording { path: PathBuf, paused: bool },
    Failed { error: crate::Error },
}

use godot::classes::IMovieWriter;

#[godot_api]
impl FelliniWriter {}

#[godot_api]
impl IMovieWriter for FelliniWriter {
    fn init(base: Base<MovieWriter>) -> Self {
        let out = Self {
            state: FelliniState::PreRecording,
            settings: Default::default(),
            base,
        };

        out
    }

    fn get_audio_mix_rate(&self) -> u32 {
        self.settings.audio_mix_rate
    }

    fn get_audio_speaker_mode(&self) -> SpeakerMode {
        SpeakerMode::STEREO
    }

    fn handles_file(&self, path: GString) -> bool {
        let path: PathBuf = path.to_string().into();
        let ext = path.extension().and_then(|s| s.to_str());
        match ext {
            Some("mp4" | "webm" | "mov") => {
                godot_print!("using fellini writer for extension {:?}", ext.unwrap());
                true
            }
            _ => false,
        }
    }
    fn write_begin(
        &mut self,
        movie_size: Vector2i,
        fps: u32,
        base_path: GString,
    ) -> godot::global::Error {
        macro_rules! gd_unwrap {
            ($f:expr) => {
                match $f {
                    Ok(ok) => ok,
                    Err(e) => {
                        godot_error!(
                            "[{}:{}]-- Failed To Set Up Fellini Writer {:?}",
                            file!(),
                            line!(),
                            e
                        );
                        self.state = FelliniState::Failed {
                            error: crate::Error::Setup,
                        };
                        return GdError::FAILED;
                    }
                }
            };
        }

        gd_unwrap!(ffmpeg::init());
        let path = PathBuf::from(&base_path.to_string());

        let mut output = gd_unwrap!(format::output(&path));

        let codec =
            gd_unwrap!(ffmpeg::encoder::find(self.settings.codec).ok_or("Codec not found."));

        let stream = gd_unwrap!(output.add_stream(codec));
        let context = gd_unwrap!(ffmpeg::codec::Context::from_parameters(stream.parameters()));

        let mut enc = gd_unwrap!(context.encoder().video());
        enc.set_width(movie_size.x.unsigned_abs());
        enc.set_height(movie_size.y.unsigned_abs());
        enc.set_time_base((1i32, fps as i32));

        gd_unwrap!(output.write_header());

        godot::global::Error::OK
    }

    unsafe fn write_frame(
        &mut self,
        _frame_image: Gd<godot::classes::Image>,
        _audio_frame_block: *const c_void,
    ) -> godot::global::Error {
        godot::global::Error::OK
    }

    fn write_end(&mut self) {}
}
