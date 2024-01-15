# 🛠️ Bevy Builder

Seamlessly create complex configurations of entities (scenes, UI layouts, etc.) using a straightforward builder pattern.

# Instructions

To add to your project, simply run:

```
cargo add --git https://github.com/empathic-ai/bevy-builder.git
```

Within your app, you can use the builder like so:

```Rust
use bevy::prelude::*;
use bevy_builder::prelude::*;

fn main() {
  App::new()
    .add_startup_system(create_simple_ui)
    .run();
}

// Use the builder to create a simple sign up UI
fn create_simple_ui(mut commands: Commands) {
  commands.child().expand().backgroud_color(Color::WHITE).v_list().small_padding().with_children(|parent| {
      parent.child().input_field("Username".to_string(), InputType::Default);
      parent.child().input_field("Email".to_string(), InputType::Default);
      parent.child().input_field("Password".to_string(), InputType::Password);
  });
}
```
