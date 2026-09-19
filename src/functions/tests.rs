use super::*;

#[derive(Component, Debug, PartialEq)]
struct Clicked(u32);

#[test]
fn entity_callbacks_keep_input_local_state_and_deferred_commands() {
    let mut world = World::new();
    let entity = world.spawn_empty().id();
    let mut commands = world.commands();
    let callback = EntityFunc::new(
        &mut commands,
        |In(entity): In<Entity>, mut count: Local<u32>, mut commands: Commands| {
            *count += 1;
            commands.entity(entity).insert(Clicked(*count));
        },
    );
    callback.call(&mut commands, entity);
    callback.call(&mut commands, entity);
    world.flush();
    assert_eq!(world.get::<Clicked>(entity), Some(&Clicked(2)));
}

#[test]
fn entity_callback_preserves_fallible_output() {
    let mut world = World::new();
    let entity = world.spawn_empty().id();
    let callback = EntityFunc::new(
        &mut world.commands(),
        |In(_): In<Entity>| -> anyhow::Result<()> { anyhow::bail!("callback failed") },
    );
    world.flush();
    let result = world.run_system_with(callback.0, entity).unwrap();
    assert!(result.unwrap_err().to_string().contains("callback failed"));
}
