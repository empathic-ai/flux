use bevy::{
    ecs::{component::Tick, system::System},
    prelude::World,
};
use std::ops::{Deref, DerefMut};

/// Lifecycle bookkeeping for systems executed outside Bevy's schedule executor.
/// The initialized last_run is a sentinel, not evidence of an ancient execution.
pub(super) struct GraphSystem<S: System + ?Sized> {
    system: Box<S>,
    initial_tick: Option<Tick>,
}

impl<S: System + ?Sized> GraphSystem<S> {
    pub fn new(system: Box<S>) -> Self {
        Self {
            system,
            initial_tick: None,
        }
    }

    pub fn initialize(&mut self, world: &mut World) {
        self.system.initialize(world);
        self.initial_tick = Some(self.system.get_last_run());
    }

    pub fn check_change_tick(&mut self, now: Tick) {
        if self.initial_tick == Some(self.system.get_last_run()) {
            // Preserve Bevy's first-run semantics, including delayed starts.
            // Adapters may skip their inner system when inputs are missing:
            // keep treating it as unrun until its actual last_run advances.
            let initial = Tick::new(now.get().wrapping_sub(Tick::MAX.get()));
            self.system.set_last_run(initial);
            self.initial_tick = Some(initial);
        } else {
            self.initial_tick = None;
            self.system.check_change_tick(now);
        }
    }
}

impl<S: System + ?Sized> Deref for GraphSystem<S> {
    type Target = S;
    fn deref(&self) -> &S {
        &self.system
    }
}
impl<S: System + ?Sized> DerefMut for GraphSystem<S> {
    fn deref_mut(&mut self) -> &mut S {
        &mut self.system
    }
}
