use ffmpeg::frame;
use ffmpeg_next as ffmpeg;

use godot::classes::audio_server::SpeakerMode;
use godot::classes::MovieWriter;
use godot::global::Error as GdError;
use godot::prelude::*;
use std::ffi::c_void;
use std::path::PathBuf;

use crate::h264_encoder::H264Encoder;
use crate::Encoder;
use crate::EncoderKind;

pub struct CommonSettings {
    viewport: Option<Gd<godot::classes::Viewport>>,
    export_alpha_matte: bool,
    export_motion_vectors: bool,
}

impl Default for CommonSettings {
    fn default() -> Self {
        Self {
            viewport: None,
            export_alpha_matte: false,
            export_motion_vectors: false,
        }
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
        //let ext = path.extension().and_then(|s| s.to_str());

        let mut settings = crate::EncoderSettings::default();
        settings.frame_rate = Some((fps as i32, 1i32).into());
        let mut kind = gd_unwrap!(H264Encoder::new(
            movie_size.x as u32,
            movie_size.y as u32,
            path.clone(),
            settings
        ));

        gd_unwrap!(kind.start());

        self.state = FelliniState::Recording {
            encoder_kind: EncoderKind::H264(kind),
            paused: false,
            current_frame: 0,
        };

        godot::global::Error::OK
    }

    unsafe fn write_frame(
        &mut self,
        frame_image: Gd<godot::classes::Image>,
        _audio_frame_block: *const c_void,
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

        //let speaker_mode = self.get_audio_speaker_mode();
        //let audio_mix_rate = self.get_audio_mix_rate();

        match &mut self.state {
            FelliniState::Recording {
                paused,
                current_frame,
                encoder_kind,
                ..
            } => {
                if *paused {
                    *current_frame += 1;
                    return godot::global::Error::OK;
                }

                gd_unwrap!(encoder_kind.push_video_frame(*current_frame as usize, frame_image));

                //let samples = crate::conversion::audio_block_size_per_frame(
                //    speaker_mode,
                //    audio_mix_rate,
                //    encoder_kind.settings().frame_rate.0 as u32,
                //);
                //let speaker_mode = crate::conversion::godot_speaker_mode_to_ffmpeg(speaker_mode);
                //let sample_ty = ffmpeg::format::Sample::I32(ffmpeg::format::sample::Type::Packed);
                //let mut audio_frame = frame::Audio::new(sample_ty, samples as usize, speaker_mode);

                //let byte_ct = samples as usize * std::mem::size_of::<i32>() * 2;
                //audio_frame.data_mut(0)[..byte_ct].copy_from_slice(std::slice::from_raw_parts(
                //    audio_frame_block as *const u8,
                //    byte_ct,
                //));
                //gd_unwrap!(encoder_kind.push_audio_frame(*current_frame as usize, audio_frame));

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
