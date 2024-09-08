mod conversion;
mod writer;

use godot::engine::{Engine, MovieWriter};
use godot::prelude::*;

pub use writer::FelliniWriter;

struct FelliniMovieWriter;

enum Error {
    Setup,
    IoError,
    LimitReached,
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
