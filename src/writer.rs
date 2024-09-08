use ffmpeg::{
    codec, decoder, encoder, format, frame, log, media, picture, Dictionary, Packet, Rational,
};
use ffmpeg_next as ffmpeg;

use godot::engine::a_star_grid_2d::ExFillSolidRegion;
use godot::engine::audio_server::SpeakerMode;
use godot::engine::MovieWriter;
use godot::global::Error as GdError;
use godot::prelude::*;
use std::ffi::c_void;
use std::path::PathBuf;

use crate::conversion;

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
            codec: ffmpeg::codec::Id::MPEG4,
            max_file_size: Self::DEFAULT_MAX_FILE_SIZE,
            audio_mix_rate: Self::DEFAULT_MIX_RATE_HZ,
        }
    }
}

enum FelliniState {
    PreRecording,
    Recording {
        path: PathBuf,
        paused: bool,
        current_frame: i64,
        output: ffmpeg::format::context::Output,
        encoder: ffmpeg::encoder::Video,
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
        let mut output = gd_unwrap!(ffmpeg::format::output(&path));

        let codec = gd_unwrap!(ffmpeg::encoder::find(self.settings.codec)
            .ok_or_else(|| ffmpeg::Error::from(ffmpeg::error::EINVAL)));

        let mut stream = gd_unwrap!(output.add_stream(codec));

        let context = gd_unwrap!(ffmpeg::codec::Context::from_parameters(stream.parameters()));
        let mut enc = gd_unwrap!(context.encoder().video());

        enc.set_width(movie_size.x.unsigned_abs());
        enc.set_height(movie_size.y.unsigned_abs());
        enc.set_time_base((1, 65_535));
        enc.set_frame_rate(Some((fps as i32, 1)));
        enc.set_format(ffmpeg::format::Pixel::YUV420P);

        let codec_context = gd_unwrap!(enc.open_as(codec));
        stream.set_parameters(&codec_context);

        gd_unwrap!(output.write_header());

        self.state = FelliniState::Recording {
            path: path.clone(),
            paused: false,
            output,
            current_frame: 0,
            encoder: codec_context,
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
        match &mut self.state {
            FelliniState::Recording {
                paused,
                encoder,
                current_frame,
                output,
                ..
            } => {
                if *paused {
                    return godot::global::Error::OK;
                }

                let width = frame_image.get_width() as u32;
                let height = frame_image.get_height() as u32;
                let mut frame = frame::Video::new(ffmpeg::format::Pixel::YUV420P, width, height);

                for plane in 0..3 {
                    let color = if plane == 0 { 255 } else { 128 };
                    frame.data_mut(plane).fill(color);
                }

                frame.set_kind(picture::Type::None);
                frame.set_pts(Some(*current_frame * (65_535 / 60)));

                gd_unwrap!(encoder.send_frame(&frame));

                let mut encoded = Packet::empty();
                while let Ok(_) = encoder.receive_packet(&mut encoded) {
                    encoded.set_stream(0);
                    encoded.set_pts(Some(*current_frame * (65_535 / 60)));
                    gd_unwrap!(encoded.write_interleaved(output));
                }

                *current_frame += 1;
            }
            _ => {
                todo!("encoding error, bailing until there is a better option");
            }
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
            FelliniState::Recording {
                output,
                path,
                encoder,
                ..
            } => {
                gd_womp!(encoder.send_eof());
                gd_womp!(output.write_trailer());

                let mut encoded = Packet::empty();
                encoded.set_stream(0);
                while let Ok(_) = encoder.receive_packet(&mut encoded) {
                    gd_womp!(encoded.write_interleaved(output));
                }

                godot_print!("Finished writing {:?}", path);
                self.state = FelliniState::PreRecording;
            }
            _ => {
                let _v = 0;
                todo!("bailed, no tail to write");
            }
        }
    }
}
