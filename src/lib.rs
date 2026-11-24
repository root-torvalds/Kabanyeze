use godot::prelude::*;

mod player_controller;
mod world_builder;
mod game_manager;

struct KabanyezeExtension;

#[gdextension]
unsafe impl ExtensionLibrary for KabanyezeExtension {}
