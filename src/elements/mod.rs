use bevy::ecs::system::EntityCommand;
use bevy::utils::HashMap;
//use silk::Bindable;
use std::sync::Arc;
//use std::sync::Mutex;

use bevy::{core::Zeroable, prelude::Component};
use bevy::prelude::*;
use bevy::prelude::{Color, Vec2, Vec4};

use std::clone::Clone;
use bevy::reflect::Reflect;
use super::DEFAULT_FONT_SIZE;

pub trait CommandCloneFn: Fn(&mut Commands) + Send + Sync {
    fn clone_box(&self) -> Box<dyn CommandCloneFn + Send + Sync>;
}

impl<T> CommandCloneFn for T
where
    T: Fn(&mut Commands) + Send + Sync + Clone + 'static,
{
    fn clone_box(&self) -> Box<dyn CommandCloneFn + Send + Sync> {
        Box::new(self.clone())
    }
}

pub struct CommandFunc(Arc<dyn CommandCloneFn + Send + Sync>);

impl CommandFunc {
    pub fn new(func: impl CommandCloneFn + Send + Sync + 'static) -> Self {
        CommandFunc(Arc::new(func))
    }

    pub fn call(&self, cmd: &mut Commands) {
        (self.0)(cmd)
    }
}

impl Clone for CommandFunc {
    fn clone(&self) -> Self {
        CommandFunc(self.0.clone())
    }
}

// Commands with 2 Args

pub trait CommandCloneWithArgs2<T1>: Fn(&mut Commands, T1) + Send + Sync {
    fn clone_box(&self) -> Box<dyn CommandCloneWithArgs2<T1> + Send + Sync>;
}

impl<T1, T> CommandCloneWithArgs2<T1> for T
where
    T: Fn(&mut Commands, T1) + Send + Sync + Clone + Copy + 'static,
{
    fn clone_box(&self) -> Box<dyn CommandCloneWithArgs2<T1> + Send + Sync> {
        Box::new(*self)
    }
}

pub struct CommandFuncWithArgs2<T1>(Arc<dyn CommandCloneWithArgs2<T1> + Send + Sync>);

impl<T1> CommandFuncWithArgs2<T1> {
    pub fn new(func: impl CommandCloneWithArgs2<T1> + Send + Sync + 'static) -> Self {
        CommandFuncWithArgs2(Arc::new(func))
    }

    pub fn call(&self, arg1: &mut Commands, arg2: T1) {
        (self.0)(arg1, arg2)
    }
}

impl<T1> Clone for CommandFuncWithArgs2<T1> {
    fn clone(&self) -> Self {
        CommandFuncWithArgs2::<T1>(self.0.clone())
    }
}

// Commands with 3 Args

pub trait CommandCloneWithArgs3<T1, T2>: Fn(&mut Commands, T1, T2) + Send + Sync {
    fn clone_box(&self) -> Box<dyn CommandCloneWithArgs3<T1, T2> + Send + Sync>;
}

impl<T1, T2, T> CommandCloneWithArgs3<T1, T2> for T
where
    T: Fn(&mut Commands, T1, T2) + Send + Sync + Clone + Copy + 'static,
{
    fn clone_box(&self) -> Box<dyn CommandCloneWithArgs3<T1, T2> + Send + Sync> {
        Box::new(*self)
    }
}

pub struct CommandFuncWithArgs3<T1, T2>(Arc<dyn CommandCloneWithArgs3<T1, T2> + Send + Sync>);

impl<T1, T2> CommandFuncWithArgs3<T1, T2> {
    pub fn new(func: impl CommandCloneWithArgs3<T1, T2> + Send + Sync + 'static) -> Self {
        CommandFuncWithArgs3(Arc::new(func))
    }

    pub fn call(&self, arg1: &mut Commands, arg2: T1, arg3: T2) {
        (self.0)(arg1, arg2, arg3)
    }
}

impl<T1, T2> Clone for CommandFuncWithArgs3<T1, T2> {
    fn clone(&self) -> Self {
        CommandFuncWithArgs3::<T1, T2>(self.0.clone())
    }
}

// 1 Arg

pub trait CloneFnWith1Arg<T1>: Fn(&mut T1) + Send + Sync {
    fn clone_box(&self) -> Box<dyn CloneFnWith1Arg<T1> + Send + Sync>;
}

impl<T1, T> CloneFnWith1Arg<T1> for T
where
    T: Fn(&mut T1) + Send + Sync + Clone + 'static,
{
    fn clone_box(&self) -> Box<dyn CloneFnWith1Arg<T1> + Send + Sync> {
        Box::new(self.clone())
    }
}

pub struct FuncWith1Arg<T1>(Arc<dyn CloneFnWith1Arg<T1> + Send + Sync>);

impl<T1> FuncWith1Arg<T1> {
    pub fn new(func: impl CloneFnWith1Arg<T1> + Send + Sync + 'static) -> Self {
        FuncWith1Arg::<T1>(Arc::new(func))
    }

    pub fn call(&self, cmd: &mut T1) {
        (self.0)(cmd)
    }
}

impl<T1> Clone for FuncWith1Arg<T1> {
    fn clone(&self) -> Self {
        FuncWith1Arg(self.0.clone())
    }
}

// 2 Args

pub trait CloneFnWith2Args<T1, T2>: Fn(T1, T2) + Send + Sync {
    fn clone_box(&self) -> Box<dyn CloneFnWith2Args<T1, T2> + Send + Sync>;
}

impl<T1, T2, T> CloneFnWith2Args<T1, T2> for T
where
    T: Fn(T1, T2) + Send + Sync + Clone + Copy + 'static,
{
    fn clone_box(&self) -> Box<dyn CloneFnWith2Args<T1, T2> + Send + Sync> {
        Box::new(*self)
    }
}

pub struct FuncWithArgs2<T1, T2>(Arc<dyn CloneFnWith2Args<T1, T2> + Send + Sync>);

impl<T1, T2> FuncWithArgs2<T1, T2> {
    pub fn new(func: impl CloneFnWith2Args<T1, T2> + Send + Sync + 'static) -> Self {
        FuncWithArgs2(Arc::new(func))
    }

    pub fn call(&self, arg1: T1, arg2: T2) {
        (self.0)(arg1, arg2)
    }
}

impl<T1, T2> Clone for FuncWithArgs2<T1, T2> {
    fn clone(&self) -> Self {
        FuncWithArgs2::<T1, T2>(self.0.clone())
    }
}

// 3 Args
pub trait CloneFnWith3Args<T1, T2, T3>: Fn(T1, T2, T3) + Send + Sync {
    fn clone_box(&self) -> Box<dyn CloneFnWith3Args<T1, T2, T3> + Send + Sync>;
}

impl<T1, T2, T3, T> CloneFnWith3Args<T1, T2, T3> for T
where
    T: Fn(T1, T2, T3) + Send + Sync + Clone + Copy + 'static,
{
    fn clone_box(&self) -> Box<dyn CloneFnWith3Args<T1, T2, T3> + Send + Sync> {
        Box::new(*self)
    }
}

pub struct FuncWithArgs3<T1, T2, T3>(Arc<dyn CloneFnWith3Args<T1, T2, T3> + Send + Sync>);

impl<T1, T2, T3> FuncWithArgs3<T1, T2, T3> {
    pub fn new(func: impl CloneFnWith3Args<T1, T2, T3> + Send + Sync + 'static) -> Self {
        FuncWithArgs3(Arc::new(func))
    }

    pub fn call(&self, arg1: T1, arg2: T2, arg3: T3) {
        (self.0)(arg1, arg2, arg3)
    }
}

impl<T1, T2, T3> Clone for FuncWithArgs3<T1, T2, T3> {
    fn clone(&self) -> Self {
        FuncWithArgs3::<T1, T2, T3>(self.0.clone())
    }
}

//pub type CommandFunc = FuncWith1Arg<Commands<'static, 'static>>;
//pub type CommandFuncWithArgs2<TArgs> = FuncWithArgs2<&'static mut Commands<'static, 'static>, TArgs>;
pub type SetPropertyFunc = CommandFuncWithArgs3<Entity, Box<dyn Reflect>>;


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    UpperLeft = 0,
    UpperCenter = 1,
    UpperRight = 2,
    MiddleLeft = 3,
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
pub struct BluetoothButton {
    pub is_initialized: bool
}

#[derive(bevy::prelude::Component, Debug)]
pub struct StripePaymentElement {
    pub FormEntity: Entity,
    pub SubmitEntity: Entity
}

#[derive(bevy::prelude::Component, Debug)]
pub struct Control {
    pub name: String,
    pub Type: String,
    pub content: String,
    pub width: f32,
    pub height: f32,
    pub fixed_width: f32,
    pub fixed_height: f32,
    pub ExpandWidth: bool,
    pub ExpandHeight: bool,
    pub anchor_min: Vec2,
    pub anchor_max: Vec2,
    pub LocalPosition: Vec2,
    pub BorderRadius: Vec4,
    pub BorderWidth: i32,
    pub Pivot: Anchor,
    pub IsVisible: bool,
    // Right, bottom, left, top
    pub Padding: Vec4,
    pub FitHeight: bool,
    pub FitWidth: bool,
    pub UseBackground: bool,
    pub Scale: f32,
    pub ScrollTop: f32,
    pub IsOverflow: bool,
    pub stretch: bool
}

#[derive(Component, Debug, Default)]
pub struct Window {
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
            ExpandWidth: false,
            ExpandHeight: false,
            anchor_min: Vec2::new(0.5, 0.5),
            anchor_max: Vec2::new(0.5, 0.5),
            LocalPosition: Vec2::new(0.0, 0.0),
            BorderRadius: Vec4::splat(0.0),
            BorderWidth: 0,
            Pivot: Anchor::MiddleCenter,
            IsVisible: true,
            Padding: Vec4::new(0.0, 0.0, 0.0, 0.0),
            FitWidth: false,
            FitHeight: false,
            UseBackground: false,
            Scale: 1.0,
            ScrollTop: 0.0,
            IsOverflow: true,
            stretch: false
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
pub struct Button {
    //pub image: String,
    pub on_click: Option<CommandFunc>,
}

#[derive(Default)]
pub enum InputType {
    #[default]
    Default,
    Password,
    PhoneNumber
}

#[derive(Component)]
pub struct InputField {
    pub text: String,
    pub placeholder: String,
    pub input_type: InputType,
    pub font_size: f32,
    pub on_submitted: Option<CommandFunc>,
    pub on_focused: Option<CommandFunc>,
    pub on_unfocused: Option<CommandFunc>,
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

#[derive(Component, Debug)]
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
pub struct IFrame {}

#[derive(Component, Debug)]
pub struct Label {
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

impl Default for Label {
    fn default() -> Self {
        Label {
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
