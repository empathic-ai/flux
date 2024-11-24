<p align="center">
    <img src="splash.png" alt="Splash">
</p>
<div align="center">
    <a href="https://www.rust-lang.org"><img height=30em src="https://img.shields.io/badge/Rust-%2320232a?style=for-the-badge&logo=rust&logoColor=red&color=141414"></a>
    <a href="https://bevyengine.org"><img height=30em src="https://img.shields.io/badge/Bevy-%2320232a?style=for-the-badge&logo=bevy&logoColor=white&color=141414"></a>
    <a href="https://openai.com"><img height=30em src="https://img.shields.io/badge/OpenAI-%2320232a?style=for-the-badge&logo=openai&logoColor=white&color=141414"></a>
    <a href="https://azure.microsoft.com"><img height=30em src="https://img.shields.io/badge/Azure-%2320232a?style=for-the-badge&logo=microsoftazure&logoColor=0078D4&color=141414"></a>
</div>

# Flux

**⚠️ Still in early development. ⚠️**

Create complex configurations of entities (scenes, UI layouts, etc.) using a straightforward builder pattern.

# Instructions

To add to your project, simply run:

```
cargo add --git https://github.com/empathic-ai/flux.git
```

Within your app, you can use the builder like this:

```Rust
use bevy::prelude::*;
use flux::prelude::*;

fn main() {
  App::new()
    .add_startup_system(create_simple_ui)
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
```
