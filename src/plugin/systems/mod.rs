use crate::prelude::*;

use bevy::ecs::archetype::Archetypes;
use bevy::ecs::component::ComponentId;
use bevy::ecs::system::{EntityCommands, SystemId};
use bevy::prelude::*;
use bevy::reflect::TypeInfo::Struct;
use bevy::reflect::{TypeRegistry, ReflectMut, ReflectRef};
use bevy::utils::HashMap;
use bevy_trait_query::All;

use serde::{Deserialize, Serialize};
use std::{any::Any, sync::Arc};
use std::any::TypeId;
use common::prelude::*;
//use bevy_cobweb::prelude::*;

#[derive(Debug, Clone, Default, Component, Reflect)]
pub struct UserMessage {
}

#[derive(Event)]
pub struct SnapScrollY(pub Entity);

#[derive(Debug, Clone, Default, Component, Reflect)]
pub struct UsageView {
    pub display: String,
    pub percent: f32
}

impl Bindable for UsageView {
    fn get(&self) -> Box<dyn Reflect> {
        Box::new(self.clone())
    }
    fn set(&mut self, value: Box<dyn Reflect>) {
        self.apply(value.as_partial_reflect());
    }
}


#[derive(Debug, Clone, Component)]
pub struct AutoBinding {
    pub source_entity: Entity,
    pub source_component_id: TypeId,
    pub source_property_name: String,
    pub target_entity: Entity,
    pub target_component_id: TypeId,
    pub target_property_name: String,
}

#[derive(Debug, Clone, Component)]
pub struct AutoSourceBinding {
    pub source_entity: Entity,
    pub source_component_id: TypeId,
    pub source_property_name: String
}

#[derive(Debug, Clone, Default, Component)]
pub struct WidthLessThan {
    pub is_visible: bool,
    pub width: f32
}

#[derive(Debug, Clone, Default, Component)]
pub struct HideOnHeightLessThan(pub f32);

#[derive(Debug, Clone, Default, Resource)]
pub struct ChangedComponents {
    pub changed_components: Vec<(Entity, TypeId)>,
}

#[derive(Debug, Default, Resource)]
pub struct Bindings {
    pub bindings_by_source: HashMap<(Entity, TypeId), (Entity, TypeId)>,
    pub source_changes_by_target: HashMap<(Entity, TypeId), HashMap<String, Box<dyn Any + Send + Sync>>>
}

#[derive(Debug, Clone, Event)]
pub struct LogInEvent {
    pub email: String,
    pub password: String
}

#[derive(Debug, Clone, Event)]
pub struct LogoutEvent {
}

#[derive(Debug, Clone, Event)]
pub struct SignUpEvent {
    pub username: String,
    pub email: String,
    pub password: String
}

#[derive(Clone, Component, Default)]
pub struct BindableChanged {
}

#[derive(Clone, Component)]
pub struct OnClick {
    pub system: SystemId
}

#[derive(Clone, Component)]
pub struct OnShow {
    pub system: Option<SystemId>,
    pub was_visible: bool
}

impl Default for OnShow {
    fn default() -> Self {
        Self { system: None, was_visible: false }
    }
}

#[derive(Clone, Component)]
pub struct Shown {
}

#[derive(Clone, Component, Default, Reflect)]
pub struct SearchInput {
    pub results: Vec<String>
}

impl Bindable for SearchInput {
    fn get(&self) -> Box<dyn Reflect> {
        Box::new(self.clone())
    }
    fn set(&mut self, value: Box<dyn Reflect>) {
        self.apply(value.as_partial_reflect());
    }
}

#[derive(Clone, Component)]
pub struct OnSubmit {
    pub func: CommandFuncWithArgs2<HashMap<String, String>>
}

#[derive(Clone, Component)]
pub struct Submitted {
    //pub values: HashMap<String, String>
}

#[derive(Clone, Component, Default)]
pub struct Form {
    pub values: HashMap<String, String>
}

#[derive(Clone, Component)]
pub struct GameManager {
    pub dynamic_view: Entity
}

#[derive(Event, Clone)]
pub struct HostGameEvent {
    pub game_manager: Entity,
    pub prompt: String
}

#[derive(Clone, Component)]
pub struct DynamicView {
    pub prompt: String
}

#[derive(Clone, Component)]
pub struct Clicked {
}

#[derive(Event, Clone)]
pub struct SubmitEvent(pub Entity);

#[derive(Event, Clone)]
pub struct UpdateChatEvent(pub Entity);

#[derive(Event)]
pub struct ClickEvent(pub Entity);

#[cfg(not(target_arch = "wasm32"))]
pub fn update_route() {
}

#[cfg(not(target_arch = "wasm32"))]
pub fn route_detection() {
}

#[cfg(not(target_arch = "wasm32"))]
pub fn event_detection() {
}

#[cfg(not(target_arch = "wasm32"))]
pub fn change_detection() {
}

#[cfg(not(target_arch = "wasm32"))]
pub fn base_change_detection() {
}

#[cfg(not(target_arch = "wasm32"))]
pub fn remove_detection() {
}

#[cfg(not(target_arch = "wasm32"))]
pub fn list_change_detection() {
}

#[cfg(not(target_arch = "wasm32"))]
pub fn update_heirarchy() {
}


#[cfg(not(target_arch = "wasm32"))]
pub fn on_show_detection() {
}

#[cfg(not(target_arch = "wasm32"))]
pub fn setup() {
}
/*
pub fn get_components_for_entity<'a>(
    entity: &Entity,
    archetypes: &'a Archetypes,
) -> Option<impl Iterator<Item = ComponentId> + 'a> {
    for archetype in archetypes.iter() {
        //archetype.entities().get(index)
        //for entity in archetype.entities().iter() {
        //    entity.table_row()
        //}

        //archetype.table_id()
        if archetype.entities().iter().any(|e| e.entity() == *entity) {
            return Some(archetype.components());
        }
    }
    None
}

pub fn process(world: &mut World) {
    /*
    let mut system_state: SystemState<(Query<(Entity, &mut Binding)>)> = SystemState::new(world);

    //let mut w_3 = w_3.lock().unwrap();
    let (query) = system_state.get_mut(world);

    let mut source_component_ids:Vec<usize> = Vec::<usize>::new();

    for (entity, mut binding) in &query {
        source_component_ids.push(binding.source_component_id);
    }

    let mut system_state: SystemState<(Query<(Entity, &mut Binding)>)> = SystemState::new(world);
    */

    //let mut query = DynamicQuery::new(world, vec![FetchKind::Ref(ComponentId::new(0))], vec![FilterKind::Without(ComponentId::new(0))]);
    //assert_eq!(query.iter().count(), 1);

    //let query: EcsValueRefQuery;

    let type_registry = TypeRegistry::default();
    //let type_registry = type_registry.read();

    let archetypes = world.archetypes();
    let entities = world.iter_entities();
    //world.components().iter();
    for entity in entities {
        let components = get_components_for_entity(&entity.id(), archetypes).unwrap();
        for component in components {
            let info = world.components().get_info(component).unwrap();
            let type_id = info.type_id();
            if type_id.is_some() {
                let type_id = type_id.unwrap();
                //let id: u64 = type_id.try_into().unwrap();
                //console::log!(id.to_string());
                let type_info = type_registry.get_type_info(type_id);
                if type_info.is_some() {
                    let type_info = type_info.unwrap();
                    //console::log!(type_info.type_name());
                }
            }
            //type_data.
        }
    }

    /*
    let es:Vec<Entity> = world.iter_entities().collect();
    //world.components().iter();
    for e in es {
        for c in world.get_entity_mut(e).unwrap().archetype().components() {


            //world.get_by_id(e, c);
            //let info = world.components().get_info(c);
            //if (info.unwrap().type_id() == Some(TypeId::of::<ChatInput>())) {

            //}
        }
    }
    */

    //world.init_resource::<Binding>();
}
 */
pub struct CommandBuilder<'w, 's> {
    commands: Commands<'w, 's>,
    custom_steps: Vec<Box<dyn Fn(&mut EntityCommands)>>, // Store closures for custom steps
}

impl<'w, 's> CommandBuilder<'w, 's> {
    pub fn new(mut commands: Commands<'w, 's>) -> Self {
        Self { 
            commands: commands,
            custom_steps: Vec::new(),
        }
    }

    pub fn spawn_entity<T: Bundle>(mut self, bundle: T) -> Self {//&'b mut EntityCommands<'_, '_> {
        let entity_commands: EntityCommands<'_> = self.commands.spawn(
            bundle
        );
        self
    }

    pub fn with_children<F>(mut self, f: F) -> Self
    where
        F: FnOnce(CommandBuilder<'w, 's>),
    {/*
        self.commands.with_children(|parent| {
            let child_builder = CommandBuilder::new(parent);
            f(child_builder);
        });
         */
        self
    }
/* 
    pub fn with_children(mut self, ) -> Self {//&'b mut EntityCommands<'_, '_> {
        let entity_commands = self.commands.spawn((
            bundle
        ));
        self
    }*/

    pub fn submit_button(mut self, label: &str, color: Color, on_click: Option<CommandFunc>) -> Self {//&'b mut EntityCommands<'_, '_> {
        let entity_commands = self.commands.spawn((
            // ... your components
        ));
        
        //entity_commands.with_children(|parent| {
            // ... child components
        //});
        
        // Store the created entity with the provided key
        //self.entities.insert(key.to_string(), entity_commands.id());
        
        // Return mutable reference to EntityCommands to allow chaining
        //&mut entity_commands
        self
    }
}

pub fn process_responsive_elements(window_query: Query<(Entity, Ref<Control>, &BWindow)>,
    mut responsive_element_query: Query<(Entity, &mut Control, Option<&WidthLessThan>, Option<&HideOnHeightLessThan>), Without<BWindow>>) {

    let mut changed_size: Option<Vec2> = None;
    for (entity, control, window) in window_query.iter() {
        if control.is_changed() {
            changed_size = Some(Vec2::new(control.width, control.height));
        }
    }

    if let Some(changed_size) = changed_size {
        println!("{}", changed_size.to_string());
        for (entity, mut control, width_less_than, height_less_than) in responsive_element_query.iter_mut() {
            if let Some(width_less_than) = width_less_than {
                if width_less_than.is_visible {
                    control.is_visible = changed_size.x <= width_less_than.width;
                } else {
                    control.is_visible = changed_size.x > width_less_than.width;
                }
            }
            if let Some(height_less_than) = height_less_than {
                control.is_visible = changed_size.y > height_less_than.0;
            }
        }
    }
}