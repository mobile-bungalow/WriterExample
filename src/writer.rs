use ffmpeg::frame;
use ffmpeg_next as ffmpeg;

use godot::classes::audio_server::SpeakerMode;
use godot::classes::MovieWriter;
use godot::global::Error as GdError;
use godot::prelude::*;
use std::ffi::c_void;
use std::i16;
use std::i32;
use std::path::PathBuf;

use crate::av1_encoder::Av1Encoder;
use crate::h264_encoder::H264Encoder;
use crate::Encoder;
use crate::EncoderKind;

pub struct CommonSettings {}

impl Default for CommonSettings {
    fn default() -> Self {
        Self {}
    }
}

#[derive(GodotClass)]
#[class(base=MovieWriter)]
pub struct FelliniWriter {
    base: Base<MovieWriter>,
    state: FelliniState,
}

const DEFAULT_MIX_RATE_HZ: u32 = 44_100;

enum FelliniState {
    PreRecording,
    Recording {
        paused: bool,
        current_frame: i64,
        encoder_kind: crate::EncoderKind,
    },
    Failed {
        error: crate::Error,
    },
}

use godot::classes::IMovieWriter;

#[godot_api]
impl FelliniWriter {}

#[godot_api]
impl IMovieWriter for FelliniWriter {
    fn init(base: Base<MovieWriter>) -> Self {
        let state = if ffmpeg::init().is_ok() {
            FelliniState::PreRecording
        } else {
            FelliniState::Failed {
                error: crate::Error::Setup,
            }
        };

        let out = Self { state, base };

        out
    }

    fn get_audio_mix_rate(&self) -> u32 {
        match &self.state {
            FelliniState::Recording { encoder_kind, .. } => {
                encoder_kind.settings().audio_sample_rate
            }
            _ => DEFAULT_MIX_RATE_HZ,
        }
    }

    fn get_audio_speaker_mode(&self) -> SpeakerMode {
        SpeakerMode::STEREO
    }

    fn handles_file(&self, path: GString) -> bool {
        let path: PathBuf = path.to_string().into();
        let ext = path.extension().and_then(|s| s.to_str());
        match ext {
            Some("mp4" | "webm" | "mov" | "mkv") => {
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

        let mut settings = crate::EncoderSettings::default();
        settings.frame_rate = Some((fps as i32, 1i32).into());
        let mut kind = gd_unwrap!(Av1Encoder::new(
            movie_size.x as u32,
            movie_size.y as u32,
            path.clone(),
            settings
        ));

        gd_unwrap!(kind.start());

        self.state = FelliniState::Recording {
            encoder_kind: EncoderKind::Av1(kind),
            paused: false,
            current_frame: 0,
        };

        godot::global::Error::OK
    }

    unsafe fn write_frame(
        &mut self,
        frame_image: Gd<godot::classes::Image>,
        // actually i32
        // TODO: when this *finally* gives length information use that
        // We need to know length for the final frame, right now we get fucking doritos bag noises
        audio_frame_block: *const c_void,
    ) -> godot::global::Error {
        macro_rules! gd_unwrap {
            ($f:expr) => {
                match $f {
                    Ok(ok) => ok,
                    Err(e) => {
                        godot_error!("[{}:{}]-- Failed to Write Frame {:?}", file!(), line!(), e);
                        self.state = FelliniState::Failed {
                            error: crate::Error::Setup,
                        };
                        return GdError::FAILED;
                    }
                }
            };
        }

        let godot_speaker_mode = self.get_audio_speaker_mode();

        match &mut self.state {
            FelliniState::Recording {
                paused,
                current_frame,
                encoder_kind,
                ..
            } => {
                if *paused {
                    return godot::global::Error::OK;
                }

                gd_unwrap!(encoder_kind.push_video_frame(*current_frame as usize, frame_image));

                if encoder_kind.settings().audio_enabled {
                    let speaker_mode =
                        crate::conversion::godot_speaker_mode_to_ffmpeg(godot_speaker_mode);

                    let sample_ty =
                        ffmpeg::format::Sample::F32(ffmpeg::format::sample::Type::Packed);

                    let frame_size = encoder_kind.audio_frame_size();

                    let mut audio_frame =
                        frame::Audio::new(sample_ty, frame_size as usize, speaker_mode);

                    let block_size =
                        crate::conversion::audio_array_size(godot_speaker_mode, frame_size);

                    let signal =
                        std::slice::from_raw_parts(audio_frame_block as *const i32, block_size / 4);

                    audio_frame.data_mut(0).fill(0);
                    for (i, sample) in signal.iter().enumerate() {
                        let flt = *sample as f32 / i32::MAX as f32;
                        let i = i * 4;
                        audio_frame.data_mut(0)[i..i + 4].copy_from_slice(&flt.to_le_bytes());
                    }

                    gd_unwrap!(encoder_kind.push_audio_frame(*current_frame as usize, audio_frame));
                }

                *current_frame += 1;
            }
            _ => {}
        }
        godot::global::Error::OK
    }

    fn write_end(&mut self) {
        macro_rules! gd_womp {
            ($f:expr) => {
                match $f {
                    Ok(ok) => ok,
                    Err(e) => {
                        godot_error!("[{}:{}]-- Set Down Failed {:?}", file!(), line!(), e);
                        self.state = FelliniState::Failed {
                            error: crate::Error::Setup,
                        };
                        return;
                    }
                }
            };
        }
        match &mut self.state {
            FelliniState::Recording { encoder_kind, .. } => {
                gd_womp!(encoder_kind.finish());
                self.state = FelliniState::PreRecording;
            }
            FelliniState::Failed { error } => {
                godot_error!("Move File failed to encode, exited with error: {error:?}");
            }
            _ => {}
        }
    }
}
