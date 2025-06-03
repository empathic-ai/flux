mod interact_state;

pub use interact_state::*;
use crate::prelude::*;
use std::collections::HashMap;
use bevy::{ecs::system::SystemId, prelude::*};
use serde::{Serialize, Deserialize};
use std::clone::Clone;

#[derive(Debug, Clone, Default, Component)]
pub struct Slider {
    pub fill_entity: Option<Entity>,
    pub percent: f32
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, Copy, Reflect, PartialEq, Eq)]
pub enum Anchor {
    UpperLeft = 0,
    UpperCenter = 1,
    UpperRight = 2,
    MiddleLeft = 3,
    #[default]
    MiddleCenter = 4,
    MiddleRight = 5,
    LowerLeft = 6,
    LowerCenter = 7,
    LowerRight = 8,
}

#[derive(bevy::prelude::Component)]
pub struct GoogleLoginButton {
}

#[derive(bevy::prelude::Component, Debug, Default)]
#[require(Control)]
pub struct BluetoothButton {
    pub is_initialized: bool
}

#[derive(bevy::prelude::Component, Debug)]
pub struct StripePaymentElement {
    pub FormEntity: Entity,
    pub SubmitEntity: Entity
}

#[derive(Debug, Clone, Serialize, Deserialize, Component, Reflect, Reactive)]
pub struct Control {
    pub name: String,
    pub Type: String,
    pub content: String,
    pub width: f32,
    pub height: f32,
    pub fixed_width: f32,
    pub fixed_height: f32,
    pub expand_width: bool,
    pub ExpandHeight: bool,
    pub anchor_min: Vec2,
    pub anchor_max: Vec2,
    pub LocalPosition: Vec2,
    pub BorderRadius: Vec4,
    pub BorderWidth: i32,
    pub Pivot: Anchor,
    pub is_visible: bool,
    // Right, bottom, left, top
    pub Padding: Vec4,
    pub FitHeight: bool,
    pub FitWidth: bool,
    pub UseBackground: bool,
    pub Scale: f32,
    pub ScrollTop: f32,
    pub is_overflow: bool,
    pub stretch: bool,
    pub ignore_layout: bool,
    pub use_blur: bool,
    pub z_index: Option<u32>
}

#[derive(Component, Debug, Default)]
pub struct BWindow {
}

#[derive(Component, Debug, Default)]
pub struct Router {
    pub path: Vec<String>,
    pub params: HashMap<String, String>,
}

impl Router {
    pub fn update_path(&mut self, path: String) {
        self.path.clear();
        self.path.push(path);
    }
}

#[derive(Component, Debug)]
pub struct Route {
    pub name: String,
    //pub is_on: bool,
}

#[derive(Event)]
pub struct ShowView {
    pub params: HashMap<String, String>
}

#[derive(Component, Debug)]
pub struct ToggleImageButton {
    pub image_on: String,
    pub image_off: String,
    pub is_on: bool,
}

#[derive(Component, Debug)]
pub struct Toggle {
    pub is_on: bool,
}

impl Anchor {
    pub fn vector_from_anchor(self) -> Vec2 {
        match self {
            Anchor::UpperLeft => Vec2::new(0.0, 0.0),
            Anchor::UpperCenter => Vec2::new(0.5, 0.0),
            Anchor::UpperRight => Vec2::new(1.0, 0.0),
            Anchor::MiddleLeft => Vec2::new(0.0, 0.5),
            Anchor::MiddleCenter => Vec2::new(0.5, 0.5),
            Anchor::MiddleRight => Vec2::new(1.0, 0.5),
            Anchor::LowerLeft => Vec2::new(0.0, 1.0),
            Anchor::LowerCenter => Vec2::new(0.5, 1.0),
            Anchor::LowerRight => Vec2::new(1.0, 1.0),
        }
    }
}

impl Default for Control {
    fn default() -> Self {
        Control {
            name: "".to_string(),
            Type: "div".to_string(),
            content: "".to_string(),
            width: 0.0,
            height: 0.0,
            fixed_width: -1.0,
            fixed_height: -1.0,
            expand_width: false,
            ExpandHeight: false,
            anchor_min: Vec2::new(0.5, 0.5),
            anchor_max: Vec2::new(0.5, 0.5),
            LocalPosition: Vec2::new(0.0, 0.0),
            BorderRadius: Vec4::splat(0.0),
            BorderWidth: 0,
            Pivot: Anchor::MiddleCenter,
            is_visible: true,
            Padding: Vec4::new(0.0, 0.0, 0.0, 0.0),
            FitWidth: false,
            FitHeight: false,
            UseBackground: false,
            Scale: 1.0,
            ScrollTop: 0.0,
            is_overflow: true,
            stretch: false,
            ignore_layout: false,
            use_blur: false,
            z_index: None
        }
    }
}

impl Control {
    pub fn stretch(&mut self) {
        self.anchor_max = Vec2::new(1.0, 1.0);
        self.anchor_min = Vec2::new(0.0, 0.0);
    }
}

#[derive(Component, Debug, Default)]
pub struct Container {}

#[derive(Component, Debug)]
pub struct VList {
    pub spacing: f32,
    /// Sets whether flex items are forced onto one line or can wrap onto multiple lines.
    pub wrapped: bool,
    pub anchor: Anchor,
    pub stretch_children: bool,
}

impl Default for VList {
    fn default() -> Self {
        VList {
            spacing: 0.0,
            wrapped: false,
            anchor: Anchor::MiddleCenter,
            stretch_children: false,
        }
    }
}

#[derive(Component, Debug, Default)]
pub struct GridList {
    pub spacing: u32,
}

#[derive(Component, Debug, Default)]
pub struct ImageButton {
    pub image: Option<Entity>,
}

#[derive(Component, Default)]
pub struct BButton {
    //pub image: String,
    pub on_click: Option<SystemId>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, Copy, Reflect, PartialEq, Eq)]
pub enum InputType {
    #[default]
    Default,
    Password,
    PhoneNumber
}

#[derive(Debug, Clone, Serialize, Deserialize, Component, Reflect, Reactive)]
pub struct InputField {
    pub text: String,
    pub placeholder: String,
    pub input_type: InputType,
    pub font_size: f32,
    #[reflect(ignore)]
    #[serde(skip)]
    pub on_submitted: Option<SystemId>,
    #[reflect(ignore)]
    #[serde(skip)]
    pub on_focused: Option<SystemId>,
    #[reflect(ignore)]
    #[serde(skip)]
    pub on_unfocused: Option<SystemId>,
    pub alignment: Anchor
}

impl Default for InputField {
    fn default() -> Self {
        InputField {
            text: "".to_string(),
            placeholder: "".to_string(),
            input_type: InputType::Default,
            font_size: 16.0,
            on_submitted: None,
            on_focused: None,
            on_unfocused: None,
            alignment: Anchor::MiddleLeft
        }
    }
}

#[derive(Component, Debug)]
pub struct HList {
    pub spacing: f32,
    pub wrapped: bool,
    pub anchor: Anchor,
    pub stretch_children: bool,
}

impl Default for HList {
    fn default() -> Self {
        HList {
            spacing: 0.0,
            wrapped: false,
            anchor: Anchor::MiddleCenter,
            stretch_children: false,
        }
    }
}

#[derive(Component, Debug, Default)]
pub struct Shadow {}

#[derive(Debug, Clone, Serialize, Deserialize, Component, Reflect, Reactive)]
pub struct ImageRect {
    pub image: String,
    pub data: Vec<u8>,
    pub brightness: f32,
    pub multiply: bool,
    pub cover_background: bool,
    pub aspect_ratio: Option<f32>,
    pub is_nine_slice: bool,
    pub border_image_slice: Vec4,
    pub border_image_width: Vec4,
}

impl Default for ImageRect {
    fn default() -> Self {
        ImageRect {
            image: "".to_string(),
            data: vec![],
            brightness: 1.0,
            multiply: false,
            cover_background: false,
            aspect_ratio: None,
            is_nine_slice: false,
            border_image_slice: Vec4::ZERO,
            border_image_width: Vec4::ZERO
        }
    }
}

#[derive(Component, Debug)]
pub struct VScroll {}

#[derive(Component, Debug)]
pub struct HScroll {}

#[derive(Component, Debug)]
pub struct IFrame {}

#[derive(Debug, Clone, Serialize, Deserialize, Component, Reflect, Reactive)]
pub struct TextLabel {
    pub text: String,
    pub font: String,
    pub font_size: f32,
    pub alignment: Anchor,
    pub color: Color,
    pub is_single_line: bool,
    pub is_bold: bool,
    pub is_italic: bool,
    pub is_shadow: bool,
    pub is_3d: bool,
    pub font_weight: f32,
    pub line_height: Option<f32>
}

impl Default for TextLabel {
    fn default() -> Self {
        TextLabel {
            text: "".to_string(),
            font: "Arial".to_string(),
            font_size: DEFAULT_FONT_SIZE,
            alignment: Anchor::MiddleCenter,
            color: Color::BLACK,
            is_single_line: false,
            is_bold: false,
            is_italic: false,
            is_shadow: false,
            is_3d: false,
            font_weight: 400.0,
            line_height: None
        }
    }
}

/*
impl Node {
    pub fn new() -> Self {
        Self {
            parent: None,
            children: Mutex::new(Vec::<Node>::new())
        }
    }

    pub fn add_child(&mut self, node: Node) {
        self.children.lock().unwrap().push(node);
        //self.children.push(node);
    }
}

trait TNode {

}

impl TNode for Node {

}

pub struct Component<T> {
    parent: Option<Box<Node>>,
    children: Mutex<Vec<Node>>,
    value: T
}

impl<T> TNode for Component<T> {

}
*/
