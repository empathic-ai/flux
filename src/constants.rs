use bevy::prelude::*;
use lazy_static::lazy_static;

pub const DEFAULT_SIZE: f32 = 400.0;
pub const EXTRA_EXTRA_LARGE: f32 = 180.0;
pub const EXTRA_LARGE: f32 = 160.0;
pub const LARGE: f32 = 80.0;
pub const MEDIUM_LARGE: f32 = 60.0;
pub const MEDIUM: f32 = 50.0;
pub const DEFAULT_FONT_SIZE: f32 = 16.0;
pub const SMALL: f32 = 25.0;
pub const SMALL_SPACE: f32 = 15.0;
pub const HALF_SMALL_SPACE: f32 = 7.5;

lazy_static! {
    pub static ref BLUE: Color = {
        Srgba::hex("0097F2").unwrap().into()
    };
    pub static ref ORANGE: Color = {
        Srgba::hex("ff8400").unwrap().into()
    };
    pub static ref GREEN: Color = {
        Srgba::hex("1db951").unwrap().into()
    };
}

pub fn get_secondary_brightness(color: Color) -> f32 {
    if color == Color::WHITE {
        0.2
    } else {
        1.0
    }
}

pub fn get_secondary_color(color: Color) -> Color {
    if color == Color::WHITE {
        Color::BLACK
    } else {
        Color::WHITE
    }
}