use bevy::prelude::*;
use bevy_builder::prelude::*;

fn main() {
  App::new()
    .add_systems(Startup, create_simple_ui)
    .run();
}

// Use the builder to create a simple sign up UI
fn create_simple_ui(mut commands: Commands) {
  commands.child().expand().v_list().small_padding().with_children(|parent| {
      parent.child().input_field("Username".to_string(), InputType::Default);
      parent.child().input_field("Email".to_string(), InputType::Default);
      parent.child().input_field("Password".to_string(), InputType::Password);
  });
}