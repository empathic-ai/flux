//! Text widgets, including fallible parsing without discarding raw input.
use super::*;

/// Field-level parse feedback, separate from the typed item draft.
#[derive(Component, Reflect, Clone, Default, PartialEq)]
pub struct EditInputStatus {
    pub message: String,
}
impl Reactive for EditInputStatus {}

pub(super) trait InputDriver: Send + Sync {
    fn pull(&mut self, world: &mut World);
    fn push(&mut self, world: &mut World);
}
struct TextInputDriver<T> {
    input: Entity,
    session: Entity,
    get: Box<dyn Fn(&T) -> String + Send + Sync>,
    set: Box<dyn Fn(&mut T, String) -> std::result::Result<(), EditError> + Send + Sync>,
    last: Option<String>,
    invalid_epoch: Option<u64>,
}
impl<T: Clone + Send + Sync + 'static> InputDriver for TextInputDriver<T> {
    fn pull(&mut self, world: &mut World) {
        let Some(input) = world.get::<crate::prelude::InputField>(self.input) else {
            return;
        };
        if self.last.as_ref().is_none_or(|last| *last == input.text) {
            return;
        }
        let text = input.text.clone();
        if let Some(mut session) = world.get_mut::<EditSession<T>>(self.session) {
            if session.cancel || session.policy == EditPolicy::DisplayOnly {
                return;
            }
            if !session.active && session.policy == EditPolicy::Live {
                let _ = session.begin();
            }
            if !session.active {
                return;
            }
            let mut candidate = session.value.clone();
            match (self.set)(&mut candidate, text.clone()) {
                Ok(()) => {
                    let _ = session.edit(|value| *value = candidate);
                    session.invalid_inputs.remove(&self.input);
                    if self.invalid_epoch.is_some()
                        && session.invalid_inputs.is_empty()
                        && session.error == Some(EditError::Invalid)
                    {
                        session.error = None;
                    }
                    self.invalid_epoch = None;
                }
                Err(_) => {
                    // Keep unparseable text in the widget, not in the typed draft.
                    session.invalid_inputs.insert(self.input);
                    session.error = Some(EditError::Invalid);
                    self.invalid_epoch = Some(session.epoch);
                    self.last = Some(text);
                }
            }
        }
    }
    fn push(&mut self, world: &mut World) {
        let value = world.get::<EditSession<T>>(self.session).map(|s| {
            let read_only = s.policy == EditPolicy::DisplayOnly
                || (s.policy == EditPolicy::Manual && !s.active);
            let invalid = self.invalid_epoch == Some(s.epoch) && s.active;
            ((self.get)(s.value()), read_only, invalid)
        });
        if let Some(mut input) = world.get_mut::<crate::prelude::InputField>(self.input) {
            if let Some((text, read_only, invalid)) = value {
                if !invalid {
                    if input.text != text {
                        input.text = text.clone();
                    }
                    self.last = Some(text);
                    self.invalid_epoch = None;
                }
                if input.read_only != read_only {
                    input.read_only = read_only;
                }
            } else if !input.read_only {
                input.read_only = true;
            }
        }
        if world.get_entity(self.input).is_ok() {
            let status = EditInputStatus {
                message: if self.invalid_epoch.is_some() {
                    EditError::Invalid.to_string()
                } else {
                    String::new()
                },
            };
            if world.get::<EditInputStatus>(self.input) != Some(&status) {
                world.entity_mut(self.input).insert(status);
            }
        }
    }
}
/// Connect text directly to a session field. Manual sessions must begin first.
/// No ordinary one-way binding may also write this input.
pub fn install_edit_input<T: Clone + Send + Sync + 'static>(
    world: &mut World,
    input: Entity,
    session: Entity,
    get: impl Fn(&T) -> String + Send + Sync + 'static,
    set: impl Fn(&mut T, String) + Send + Sync + 'static,
) -> Result<()> {
    install_edit_input_try(world, input, session, get, move |value, text| {
        set(value, text);
        Ok(())
    })
}
/// Fallible conversion retains invalid text, blocks Save, and exposes EditInputStatus.
/// Cancel discards the raw input and reloads the latest source value.
pub fn install_edit_input_try<T: Clone + Send + Sync + 'static>(
    world: &mut World,
    input: Entity,
    session: Entity,
    get: impl Fn(&T) -> String + Send + Sync + 'static,
    set: impl Fn(&mut T, String) -> std::result::Result<(), EditError> + Send + Sync + 'static,
) -> Result<()> {
    ensure!(
        world.get::<crate::prelude::InputField>(input).is_some(),
        "Expected an InputField"
    );
    ensure!(
        world.contains_resource::<EditBindings>(),
        "Add EditBindingPlugin first"
    );
    ensure!(
        world.get::<EditInputOwner>(input).is_none(),
        "An editor already owns this input"
    );
    world.entity_mut(input).insert(EditInputOwner);
    let mut driver = TextInputDriver {
        input,
        session,
        get: Box::new(get),
        set: Box::new(set),
        last: None,
        invalid_epoch: None,
    };
    driver.push(world);
    world
        .resource_mut::<EditBindings>()
        .inputs
        .insert(input, Box::new(driver));
    Ok(())
}
