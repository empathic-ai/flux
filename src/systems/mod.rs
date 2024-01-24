use crate::prelude::*;

use bevy::ecs::archetype::Archetypes;
use bevy::ecs::component::ComponentId;
use bevy::ecs::system::EntityCommands;
use bevy::prelude::*;
use bevy::reflect::TypeInfo::Struct;
use bevy::reflect::{TypeRegistry, ReflectMut, ReflectRef};
use bevy::utils::HashMap;
use bevy_trait_query::All;

use serde::{Deserialize, Serialize};
use std::{any::Any, sync::Arc};
use std::any::TypeId;
use common::prelude::*;


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
        self.apply(value.as_reflect());
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
    pub func: CommandFunc
}

#[derive(Clone, Component)]
pub struct OnShow {
    pub func: Option<CommandFunc>,
    pub was_visible: bool
}

impl Default for OnShow {
    fn default() -> Self {
        Self { func: None, was_visible: false }
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
        self.apply(value.as_reflect());
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
    let type_registry = type_registry.read();

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
        let entity_commands: EntityCommands<'_, '_, '_> = self.commands.spawn(
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

pub fn process_responsive_elements(window_query: Query<(Entity, &Control, &Window, Changed<Control>)>,
    mut responsive_element_query: Query<(Entity, &mut Control, Option<&WidthLessThan>, Option<&HideOnHeightLessThan>), Without<Window>>) {

    let mut changed_size: Option<Vec2> = None;
    for (entity, control, window, changed_control) in window_query.iter() {
        if changed_control {
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

// TODO: rewrite to propogate bindings until a queue of all binding events is emptied
// 1. Create queue from bindable_struct_query
// 2. Apply queue using property_query--add changes to bindable_structs to queue
// 3. Loop until queue is empty
// (Avoid using SetPropertyFunc in the future, unless necessary--using commands can cause delay in processing)
// (Instead of using SetPropertyFunc, define a value 'transformation')
// (i.e. a list of messages could be transformed to a List of tuples for improved-styling--(is_first, message))
// (subsequent use of tuple works because of dynamic property binding using Box<dyn Reflect>)

// Move conflicting queries into a ParamSet: https://bevy-cheatbook.github.io/programming/paramset.html
pub fn propogate_forms(
    mut commands: Commands,
    //type_registry: Res<AppTypeRegistry>,
    //mut ev_reader: EventReader<SubmitEvent>,
    //mut label_query: Query<(&mut Label, &AutoBindableProperty)>,
    mut set: ParamSet<(
        Query<(Entity, All<&dyn Bindable>), Or<(With<BindableChanged>, Changed<AutoBindable>)>>,
        Query<(Entity, Option<&mut Control>, Option<&mut BLabel>, Option<&mut ImageRect>, Option<&mut Slider>, Option<&mut InputField>, Option<&mut BackgroundColor>, &AutoBindableProperty, Option<&mut AutoBindable>)>
    )>,
    mut auto_bindable_list_query: Query<(Entity, &AutoBindableList, Option<&Children>)>,
    //form_query: Query<(Entity, &Form, Option<Changed<Form>>)>,
) {
    let mut binding_queue = HashMap::<Entity, HashMap::<String, Box<dyn Reflect>>>::new();

    for (entity, bindables) in set.p0().iter() {//bindable_struct_query.iter() {
        binding_queue.insert(entity, HashMap::<String, Box<dyn Reflect>>::new());

        commands.entity(entity).remove::<BindableChanged>();
        for bindable in bindables {
            
            //if was_changed.is_some_and(|x| x) {
            //if let Some(chat_view) = form.as_any().downcast_ref::<DetailedChatView>() {
            //}
            let reflect = bindable.get();
            
            //let mut field_values = HashMap::<String, Box<dyn Reflect>>::new();

            //console::log!("IS BINDABLE STRUCT".to_string());
            /*
            if let Some(reflect) = form.get().downcast_ref::<Box<dyn Reflect>>() {
                console::log!("GOT IT".to_string());

                let registry = type_registry.0.internal.read();
                let serializer = ReflectSerializer::new(&(**reflect), &registry);
    
                let serialized_value: String = ron::to_string(&serializer).unwrap();
                
                console::log!(serialized_value.clone());

                if let Ok(ron_value) = ron::de::from_str::<ron::Value>(&serialized_value) {

                }
            }
            */
            
            let reflect_ref = reflect.reflect_ref();
            //if let Some(reflect) = form.as_any().downcast_ref::<&'static dyn Reflect>() {
                //let reflect = Box::new(reflect);
            if let ReflectRef::Struct(value) = reflect_ref {
                //console::log!("IS STRUCT".clone());

                if let Some(type_info) = value.get_represented_type_info() {//type_registry.0.read().get_type_info(form.type_id()) {
                    
                    //console::log!(type_info.type_name().clone());

                    if let Struct(struct_info) = type_info {
                        for name in struct_info.field_names() {
                            //console::log!(name.clone());
                            if let Some(value) = value.clone().field(name) {
                                let value = value.clone_value();
                                if let Some(mut field_values) = binding_queue.get_mut(&entity) {
                                    let _name = name.clone();
                                    //console::log!(format!("PROPOGATING {_name}"));
                                    field_values.insert(name.to_string(), value);
                                }
                            }
                        }
                    }
                }
            } else if let ReflectRef::Value(value) = reflect_ref {
                // TODO: Handle
                if let Some(mut field_values) = binding_queue.get_mut(&entity) {
                    let type_name = value.type_name();
                    let id = entity.to_bits().to_string();
                    //console::log!(format!("PROPOGATING VALUE OF TYPE: {type_name} FROM: {id}"));
                    let value = value.clone_value();
                    field_values.insert("".to_string(), value);
                }
            }
        }
    }

    for (entity, bindable_list, children) in auto_bindable_list_query.iter_mut() {
        if let Some(field_values) = binding_queue.get(&bindable_list.entity) {
            if let Some(property_value) = field_values.get(&bindable_list.property_name) {
                let property_name = bindable_list.property_name.clone();
                //console::log!(format!("UPDATING BINDABLE LIST: {property_name}"));
                
                if let Some(children) = children {
                    let children = children.to_vec();
                    for child in children {
                        commands.entity(child).despawn_recursive();
                    }
                }
                if let ReflectRef::Array(value) = property_value.reflect_ref() {
                }
                if let ReflectRef::List(value) = property_value.reflect_ref() {
                    for item in value.iter() {
                        let mut item = item;
                        if let ReflectRef::Value(value) = item.reflect_ref() {
                            item = value;
                            //console::log!(item.type_name());
                        }
                        let child = bindable_list.create_entity.as_ref().unwrap().call(&mut commands);
                        let item = item.clone_value();
                        commands.entity(entity).add_child(child).add(move |id: Entity, world: &mut World| {
                            if let Some(mut bindable_struct) = world.entity_mut(child).get_mut::<AutoBindable>() {
                                Bindable::set(bindable_struct.as_mut(), item);
                            }
                            //if let Some(mut bindable_property) = world.entity_mut(child).get_mut::<AutoBindableProperty>() {
                                // TODO: run separate code
                            //}
                            //if !is_value {
                            world.entity_mut(child).insert(BindableChanged {});
                        });
                    }
                    //commands.entity(entity).remove::<AutoBindableList>();
                }
            }
        }
    }

    // TODO: simply forward property value to bindable_struct. All bindable_property's will do this
    for (property_entity, mut control, mut label, mut image_rect, mut slider, mut input_field, mut background_color, bindable_property, bindable) in set.p1().iter_mut() {
        if let Some(field_values) = binding_queue.get(&bindable_property.entity) {
            let property_name = bindable_property.property_name.clone();
            if let Some(property_value) = field_values.get(&property_name) {
                if bindable_property.property_name != "" && bindable.is_some() {
                    let mut bindable = bindable.unwrap();

                    bindable.set(property_value.clone_value());
                    commands.add(move|world: &mut World| {
                        if let Some(mut entity) = world.get_entity_mut(property_entity) {
                            entity.insert(BindableChanged {});
                        }
                    });
                } else if let Some(func) = &bindable_property.entity_func {
                    func.call(&mut commands, property_entity, property_value.clone_value());
                } else {
                    if let Some(mut input_field) = input_field {
                        if let Some(value) = property_value.downcast_ref::<String>() {
                            input_field.text = value.clone();
                        }
                    }
                    if let Some(mut background_color) = background_color {
                        if let Some(value) = Color::from_reflect(property_value.as_reflect()) {
                            background_color.0 = value.clone();
                        }
                    }
                    if let Some(mut slider) = slider {
                        if let Some(value) = property_value.downcast_ref::<f32>() {
                            slider.percent = value.clone();
                        }
                    }
                    if let Some(value) = property_value.downcast_ref::<Vec<u8>>() {
                        log(property_name);
                        log("IS BYTE ARRAY");
                    }
                    if let Some(value) = property_value.downcast_ref::<String>() {
                        if let Some(mut label) = label {
                            label.text = value.to_owned();
                        }
                        if let Some(mut image_rect) = image_rect {
                            image_rect.image = value.to_owned();
                        }
                    }
                    if let Some(mut control) = control {
                        if let Some(value) = property_value.downcast_ref::<bool>() {
                            let _value = value.clone();
                            control.is_visible = value.to_owned();
                            let property_name = bindable_property.property_name.clone();
                            //console::log!(format!("SET BINDABLE VALUE: {property_name}"));
                            //console::log!(format!("IS VISIBLE: {_value}"));
                        }
                    }
                }
            }
        }
    }
}



pub fn process_form_on_submit(
    mut commands: Commands,
    mut ev_reader: EventReader<SubmitEvent>,
    mut struct_query: Query<(Entity, All<&mut dyn Bindable>)>,
    input_query: Query<(&InputField, &AutoBindableProperty), (Changed<InputField>)>,
    mut form_query: Query<(Entity, Option<&OnSubmit>)>,
) {
    for (input_field, bindable_property) in input_query.iter() {
        if let Ok((entity, bindables)) = struct_query.get_mut(bindable_property.entity) {
            for mut bindable in bindables {

                let mut reflect = bindable.get();
                let reflect_ref = reflect.reflect_mut();
    
                if let ReflectMut::Struct(value) = reflect_ref {
                    if let Some(field) = value.field_mut(&bindable_property.property_name) {
                        field.set(Box::new(input_field.text.clone()));
                    }
                }
                bindable.set(reflect);
            }
        }
    }

    for ev in ev_reader.iter() {
        commands.entity(ev.0).insert(Submitted { });
    }
    /*
    for ev in ev_reader.iter() {
        let entity = ev.0.to_bits().to_string();
        //console::log!(format!("GOT SUBMIT EVENT! Entity: {entity}"));
        //if *interaction == Interaction::Clicked {
        // Get all input fields and form context associated with this form_id
        let inputs: Vec<(&InputField, &AutoBindableProperty)> = input_query.iter().filter(|(input_field, data_binding)| data_binding.entity == ev.0).collect();
        let mut form_result = form_query.get(ev.0);

        // Process the form with the captured inputs and context
        if let Ok((entity, on_submit)) = form_result {
            let mut args = HashMap::<String, String>::new();
            for (input_field, data_binding) in inputs {
                let form_id = ev.0.to_bits().to_string();
                let property_name = data_binding.property_name.clone();
                let property_value = input_field.text.clone();
                args.insert(property_name.clone(), property_value.clone());
                crate::prelude::log(format!("Getting form {form_id} input. Field: {property_name} Value: {property_value}"));
            }
            //console::log!("CALLING ON SUBMIT FUNCTION!");
            if let Some(on_submit) = on_submit {
                on_submit.func.call(&mut commands, args.clone());
            }
            commands.entity(entity).insert(Submitted { values: args });
        }
    }
     */
}