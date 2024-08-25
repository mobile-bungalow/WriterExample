use godot::engine::audio_server::SpeakerMode;
use godot::engine::MovieWriter;
use godot::prelude::*;
use std::ffi::c_void;
use std::path::PathBuf;

#[derive(GodotClass)]
#[class(base=MovieWriter)]
pub struct FelliniWriter {
    base: Base<MovieWriter>,
    audio_mix_rate: u32,
}

use godot::classes::IMovieWriter;

#[godot_api]
impl IMovieWriter for FelliniWriter {
    fn init(base: Base<MovieWriter>) -> Self {
        let out = Self {
            base,
            audio_mix_rate: 0,
        };

        out
    }

    fn get_audio_mix_rate(&self) -> u32 {
        self.audio_mix_rate
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
        _movie_size: Vector2i,
        _fps: u32,
        _base_path: GString,
    ) -> godot::global::Error {
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
