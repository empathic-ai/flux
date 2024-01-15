use std::sync::Arc;
use bevy::{prelude::*, utils::HashMap};


pub type SubmitFunc = CommandFuncWithArgs2<HashMap<String, String>>;

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


pub trait EntityCloneFn: Fn(&mut Commands) -> Entity + Send + Sync {
    fn clone_box(&self) -> Box<dyn EntityCloneFn + Send + Sync>;
}

impl<T> EntityCloneFn for T
where
    T: Fn(&mut Commands) -> Entity + Send + Sync + Clone + 'static,
{
    fn clone_box(&self) -> Box<dyn EntityCloneFn + Send + Sync> {
        Box::new(self.clone())
    }
}

pub struct CreateEntityFunc(Arc<dyn EntityCloneFn + Send + Sync>);

impl CreateEntityFunc {
    pub fn new(func: impl EntityCloneFn + Send + Sync + 'static) -> Self {
        CreateEntityFunc(Arc::new(func))
    }

    pub fn call(&self, cmd: &mut Commands) -> Entity {
        (self.0)(cmd)
    }
}

impl Clone for CreateEntityFunc {
    fn clone(&self) -> Self {
        CreateEntityFunc(self.0.clone())
    }
}
