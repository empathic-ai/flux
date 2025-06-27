use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};

use crate::prelude::*;

use bevy::ecs::error::HandleError;
use bevy::ecs::system::{command, SystemParam};
use bevy::prelude::*;
use bevy::reflect::{DynamicEnum, ParsedPath, Reflect, ReflectFromPtr, ReflectMut, ReflectRef, Struct, TypeRegistry};
use bevy_trait_query::All;

use common::prelude::*;
use anyhow::{anyhow, Result};

#[derive(Event, Clone)]
pub struct OnChange {
    pub entity: Entity
}

#[derive(Resource, Default)]
pub struct BindingsConfig {
    pub source_bindings: HashMap<Entity, HashSet<Entity>>,
    pub target_bindings: HashMap<Entity, HashSet<Entity>>,
    pub type_registry: TypeRegistry
    //pub bindings: HashMap<Uuid, Binding>
}

impl BindingsConfig {
    pub fn update_binding(&mut self, entity: Entity, binding: Binding) {
        //let binding_id = Uuid::new_v4();
        //self.bindings.insert(binding_id, binding.clone());
        
        match binding {
            Binding::Path(binding) => {
                if let Some(source_entity) = binding.source_entity {
                    self.source_bindings.entry(source_entity).or_default().insert(entity);
                }
        
                if let Some(target_entity) = binding.target_entity {
                    self.target_bindings.entry(target_entity).or_default().insert(entity);
                }
            }
            Binding::List(binding) => {
                if let Some(source_entity) = binding.source_entity {
                    self.source_bindings.entry(source_entity).or_default().insert(entity);
                }
        
                if let Some(target_entity) = binding.target_entity {
                    self.target_bindings.entry(target_entity).or_default().insert(entity);
                }
            },
        }
    }

    pub fn get_target_bindings(&mut self, entity: Entity) -> &HashSet<Entity> {
        self.target_bindings.entry(entity).or_insert(HashSet::new())
    }

    pub fn get_source_bindings(&mut self, entity: Entity) -> &HashSet<Entity> {
        self.source_bindings.entry(entity).or_insert(HashSet::new())
    }
}

#[derive(SystemParam)]
pub struct Bindings<'w, 's> {
    commands: Commands<'w, 's>,
    config: ResMut<'w, BindingsConfig>,
    pub bindings: Query<'w, 's, (Entity, Mut<'static, Binding>)>,
    pub reactives: Query<'w, 's, (Entity, All<&'static mut dyn Reactive>)>
}

impl<'w, 's> Bindings<'w, 's> {

    pub fn update(&mut self) {
        let mut changed_components = HashSet::new();

        for (entity, binding) in self.bindings.iter() {
            if binding.is_added() || binding.is_changed() {
                self.config.update_binding(entity, binding.clone());
                Self::apply_binding_internal(&mut self.commands, &mut self.config.type_registry, &mut self.reactives, binding.clone(), &mut changed_components);
                //self.update_binding(entity, binding.clone());
            }
        }

        let mut changed_bindings = HashSet::new();
        for (entity, reactives) in self.reactives.iter() {
            for reactive in reactives.iter() {
                if reactive.is_added() || reactive.is_changed() {
                    let component_name = reactive.reflect_short_type_path();

                    //info!("Reactive added or changed: <{}>.{}", entity, component_name);

                    for binding_id in self.config.get_source_bindings(entity).iter() {
                        let (_, binding) = self.bindings.get_mut(binding_id.clone()).unwrap();
                        if binding.get_source_component_name() == component_name {
                            changed_bindings.insert(binding_id.clone());
                        }
                    }
                }
            }
        }
        for entity in changed_bindings.iter() {
            self.apply_binding(entity.clone(), &mut changed_components);
        }

        while changed_components.len() > 0 {
            let mut changed_bindings = HashSet::new();
            for (entity, component_name) in changed_components.iter() {
                for binding_id in self.config.get_source_bindings(entity.clone()).iter() {
                    let (_, binding) = self.bindings.get_mut(binding_id.clone()).unwrap();
                    if binding.get_source_component_name() == component_name.clone() {
                        //info!("Reactive added or changed: <{}>.{}", entity, component_name);
                    
                        changed_bindings.insert(binding_id.clone());
                    }
                }
            }
            changed_components.clear();
            
            for entity in changed_bindings.iter() {
                self.apply_binding(entity.clone(), &mut changed_components);
            }
        }
    }

    pub fn apply_binding(&mut self, entity: Entity, changed_reactives: &mut HashSet<(Entity, String)>) {
        let binding = self.get_binding(entity).clone();

        Self::apply_binding_internal(&mut self.commands, &mut self.config.type_registry, &mut self.reactives, binding, changed_reactives);
    }

    pub fn add_binding(&mut self, binding: Binding) {
        self.commands.spawn(binding);
    }
    
    //pub fn update_binding(&mut self, entity: Entity, binding: Binding) {
    //    self.config.update_binding(entity, binding.clone());
    //    Self::apply_binding_internal(&mut self.commands, &mut self.reactives, binding);
    //}
    
    /*
    pub fn get_source_bindings_with_component<'a>(bindings: &'a mut Query<'w, 's, (Entity, Mut<'static, Binding>), ()>, config: &'a mut BindingsConfig, entity: Entity, component_name: String) -> Box<dyn Iterator<Item = &'a Mut<'a, Binding>> + 'a> where 'w: 'a, 's: 'a {
        //let bindings = &'a mut self.bindings;
        let component_name = component_name.clone();

        Box::new(config.get_source_bindings(entity).iter().map(|entity| {
                    let (_, binding) = bindings.get_mut(entity.clone()).unwrap();
                    binding
                }).filter(move |binding| {
                    (binding.get_source_component_name() == component_name)
                }))
    } */

    pub fn get_source_bindings(&mut self, entity: Entity) -> &HashSet<Entity> {
        self.config.get_source_bindings(entity)
    }

    pub fn get_target_bindings(&mut self, entity: Entity) -> &HashSet<Entity> {
        self.config.get_target_bindings(entity)
    }

    pub fn get_binding(&mut self, entity: Entity) -> Mut<'_, Binding> {
        let (_, mut binding) = self.bindings.get_mut(entity).unwrap();
        binding
    }

    fn get_source_value(reactives: &mut Query<'w, 's, (Entity, All<&'static mut dyn Reactive>)>, entity : Entity, component_name: String, property_path: Option<String>) -> Option<Box<dyn PartialReflect>> {
        if let Ok((_, reactives)) = reactives.get(entity) {
            if let Some(reactive) = reactives.iter().find(|x| x.reflect_short_type_path() == &component_name) {
                if let Some(property_path) = property_path {
                    if let ReflectRef::Struct(reactive) = reactive.reflect_ref() {
                        if let Ok(property) = reactive.try_as_reflect().unwrap().reflect_path(&ParsedPath::parse(&property_path).unwrap()) {
                            Some(property.clone_value())
                        } else {
                            //warn!("Failed to get source value. Failed to get field in reactive with property '{}'!", property_path);
                            None
                        }
                    } else {
                        //warn!("Failed to get source value. Reactive isn't a struct!");
                        None
                    }
                } else {
                    Some(reactive.clone_value())
                }
            } else {
                //warn!("Failed to get source value. No reactive found in entity <{}> with component name '{}'!", entity, component_name);
                None
            }
        } else {
            //warn!("Failed to get source value. No reactives found in entity <{}>! Component name: {}.", entity, component_name);
            None
        }
    }

    fn apply_binding_internal(commands: &mut Commands<'w, 's>, type_registry: &mut TypeRegistry, reactives: &mut Query<'w, 's, (Entity, All<&'static mut dyn Reactive>)>, binding: Binding, changed_components: &mut HashSet<(Entity, String)>) -> Result<()> {
        //let mut binding = self.get_binding(binding_id).clone();

        let source_component_name = binding.get_source_component_name();
  
        match binding {
            Binding::Path(binding) => {
                    if let Some(source_entity) = binding.source_entity {
                        if let Some(target_entity) = binding.target_entity {
        
                        if let Some(mut source_value) = Self::get_source_value(reactives, source_entity, binding.source_component_name.clone(), binding.source_property_path.clone()) {
                            if let Ok((_, mut reactives)) = reactives.get_mut(target_entity) {

                                let target_component_name = binding.target_component_name.clone();
                                if let Some(mut target_bindable) = reactives.iter_mut().find(|x| x.reflect_short_type_path() == target_component_name) {
                            
                                    let mut target_value = if let Some(property_path) = binding.target_property_path.clone() {
                                        
                                        match target_bindable.reflect_path_mut(&ParsedPath::parse(&property_path).unwrap()) {
                                            Ok(value) => {
                                                value
                                            }
                                            Err(err) => {
                                                return Err(anyhow!("Failed to get path in binding: {}. {}", binding.to_string(), err));
                                            }
                                        }
                                    } else {
                                        target_bindable.as_partial_reflect_mut()
                                    };

                                    if target_value.is_dynamic() {
                                        let target_type_name = target_value.reflect_short_type_path();
                                        //info!("Target type: {}", target_type_name);
                                        let type_registration = type_registry.get_with_short_type_path_mut(target_type_name).unwrap();
        
                                        let reflect_from_reflect = type_registration
                                        .data::<ReflectFromReflect>()
                                        .expect("`ReflectFromReflect` should be registered");
        
                                        let source_value = reflect_from_reflect
                                        .from_reflect(source_value.as_ref())
                                        .unwrap();
                                        
                                        if !target_value.reflect_partial_eq(source_value.as_partial_reflect()).unwrap_or(false) {
                                            //info!("Applying path binding: {}. Value: {:?}", binding.to_string(), source_value);

                                            //info!("Value: {:?}", source_value);
                
                                            target_value.try_as_reflect_mut().unwrap().set(source_value);

                                            //info!("New value: {:?}", target_value);
                                            
                                            changed_components.insert((target_entity, target_component_name));
                                        }
                                    } else {
                                    
                                        //info!("Source type: {}", source_value.reflect_short_type_path());
                                        //info!("Target type: {}", target_value.reflect_short_type_path());

                                        let is_option = if let Some(type_info) = source_value.get_represented_type_info() {
                                            type_info.is::<Option<String>>()
                                        } else {
                                            false
                                        };
                                        
                                        let is_option = source_value.represents::<Option<String>>();
                                        let different_types = source_value.reflect_short_type_path() != target_value.reflect_short_type_path();
                                        
                                        if is_option && different_types {
                                            if let ReflectRef::Enum(dyn_enum) = source_value.reflect_ref() {
                                                if let Some(value) = dyn_enum.field_at(0) {
                                                    source_value = value.clone_value();
                                                }
                                            }
                                        }
                
                                        if !target_value.reflect_partial_eq(source_value.as_partial_reflect()).unwrap_or(false) {
                                            //info!("Applying path binding: {}. Value: {:?}", binding.to_string(), source_value);

                                            //info!("Value: {:?}", source_value);
                
                                            target_value.apply(source_value.as_ref());

                                            //info!("New value: {:?}", target_value);
                                            
                                            changed_components.insert((target_entity, target_component_name));
                                        }
                                    }
                                }
                            }
                        } else {
                            //info!("Attempted to apply path binding but failed to get source value: {}", binding.to_string());
                        }
                    }
                } else {
                    //info!("Attempted to apply path binding but target or source is missing: {}", binding.to_string());
                }
            },
            Binding::List(binding) => {
                if let Some(source_entity) = binding.source_entity {
                    if let Some(target_entity) = binding.target_entity {
 
                        if let Some(source_value) = Self::get_source_value(reactives, source_entity, binding.source_component_name.clone(), binding.source_property_path.clone()) {
                            //if let Ok((_, mut reactives, children)) = reactives.get_mut(target_entity) {

                                if let ReflectRef::Array(value) = source_value.reflect_ref() {
                                }
                                if let ReflectRef::List(value) = source_value.reflect_ref() {
                                    //info!("Applying list binding: {}. Value: {:?}", binding.to_string(), source_value);

                                    commands.entity(target_entity.clone()).despawn_related::<Children>();

                                    let target_component_name = binding.target_component_name.clone();
                                    let target_property_path = binding.target_property_path.clone();

                                    for item in value.iter() {
                                        let mut item = item;
        
                                        let child = binding.create_entity_func.as_ref().unwrap().call(commands);
                                        let source_value = item.clone_value();
                
                                        commands.entity(target_entity).add_child(child);
            
                                        let target_component_name = target_component_name.clone();
                                        let target_property_path = target_property_path.clone();
                                        let binding = binding.clone();

                                        //.add(move |id: Entity, world: &mut World| {
                                        let system_id = commands.register_system((move |world: &mut World| {
                                            let bindings = world.resource::<BindingsConfig>();
                                            let type_registry = &bindings.type_registry;

                                            let type_registration = type_registry.get_with_short_type_path(&target_component_name).expect(&format!("Failed to get type info for component named '{}'!", target_component_name));

                                            let reflect_from_ptr = type_registration.data::<ReflectFromPtr>().unwrap().clone();

                                            let component_id = world.components().get_id(type_registration.type_id()).unwrap();

                                            if let Ok(mut component) = world.entity_mut(child).get_mut_by_id(component_id) {
                                                let mut component = unsafe { reflect_from_ptr.as_reflect_mut(component.into_inner()) };
                                                let mut target_value = if let Some(property_path) = &target_property_path {
                                            
                                                        // TODO: run separate code
                                                        let mut path = component.reflect_path_mut(&ParsedPath::parse(&property_path).unwrap()).unwrap();
                                                        path
                                                    } else {
                                                        // TODO: Automatically insert entity if it doesn't yet exist (currently must be explicitly added in create function)
                                                        //world.entity_mut(child).insert_by_id(component_id, component)
                                                        component.as_partial_reflect_mut()
                                                    };

                                                info!("{}", target_value.reflect_short_type_path());

                                                // If the values aren't equal, apply the new value to the target
                                                if !source_value.reflect_partial_eq(target_value.as_partial_reflect()).unwrap_or(false) {
                                                    /*
                                                    if target_value.reflect_short_type_path() == "Dynamic" {
                                                        info!("IS DYNAMIC!");
                                                        if let ReflectRef::Struct(struct_ref) = target_value.reflect_ref() {
                                                            info!("IS STRUCT!");
                                                        }
                                                        if let ReflectRef::Opaque(struct_ref) = target_value.reflect_ref() {
                                                            info!("IS OPAQUE!");
                                                        }
                                                        info!("Inner type: {}", target_value.get_represented_type_info().unwrap().type_path());
                                                    }*/

                                                    //info!("Applying list binding for element. Value: {:?}", source_value);
                                                    match target_value.try_apply(source_value.as_partial_reflect()) {
                                                        Ok(_) => {},
                                                        Err(err) => {
                                                            return Err(anyhow!("Failed to apply list element value of type '{}' to type '{}'. Value: {:?}\n\n{}", source_value.reflect_type_path(), target_value.reflect_type_path(), source_value, err));
                                                        },
                                                    }
                                                }
                                            } else {
                                                return Err(anyhow!("Failed to find component '{}' for binding: {}.", target_component_name, binding.to_string()));
                                            }
                                            Ok(())
                                        }));
                                        commands.queue(command::run_system(system_id).handle_error_with(bevy::ecs::error::warn));
                                        commands.unregister_system(system_id);

                                            //| world: &mut World| {
                                                /*

                                                */
                                            //});

                                                //if let Some(mut bindable_property) = world.entity_mut(child).get_mut::<AutoBindableProperty>() {
                                                    // TODO: run separate code
                                                //}
                                                //if !is_value {
                                                //world.entity_mut(child).insert(BindableChanged {});
                                            //});
                                    }
                                    //commands.entity(entity).remove::<AutoBindableList>();
                                }
                            //}
                        } else {
                            //info!("Attempted to apply binding but failed to get source value.");
                        }
                    }
                } else {
                    //info!("Attempted to apply binding but target or source is missing.");
                }
            },
        }
        Ok(())
    }
}

fn system(world: &mut World) {

}

#[derive(Component, Clone)]
pub enum Binding {
    Path(PathBinding),
    List(ListBinding)
}

impl Binding {
    pub fn set_source(&mut self, entity: Option<Entity>) {
        match self {
            Binding::Path(binding) => {
                binding.source_entity = entity
            },
            Binding::List(binding) => {
                binding.source_entity = entity
            },
        }
    }

    pub fn get_source_component_name(&self) -> String {
        match self {
            Binding::Path(binding) => {
                binding.source_component_name.clone()
            },
            Binding::List(binding) => {
                binding.source_component_name.clone()
            },
        }
    }

    pub fn to_string(&self) -> String {
        match self {
            Binding::Path(binding) => {
                binding.to_string()
            },
            Binding::List(binding) => {
                binding.to_string()
            },
        }
    }
}

#[derive(Component, Clone)]
pub struct PathBinding {
    pub source_entity: Option<Entity>,
    pub source_component_name: String,
    pub source_property_path: Option<String>,
    pub target_entity: Option<Entity>,
    pub target_component_name: String,
    pub target_property_path: Option<String>,
    pub entity_func: Option<SetPropertyFunc>
}

impl PathBinding {
    pub fn to_string(&self) -> String {
        format!("<{}>.{}{} -> <{}>.{}{}", entity_to_string(&self.source_entity), self.source_component_name, path_to_string(&self.source_property_path), entity_to_string(&self.target_entity), self.target_component_name, path_to_string(&self.target_property_path))
    }
}

fn path_to_string(property_path: &Option<String>) -> String {
    if let Some(property_path) = property_path {
        format!(".{}", property_path)
    } else {
        "".to_string()
    }
}

fn entity_to_string(entity: &Option<Entity>) -> String {
    if let Some(entity) = entity {
        entity.to_string()
    } else {
        "?".to_string()
    }
}

#[derive(Component, Clone)]
pub struct ListBinding {
    pub source_entity: Option<Entity>,
    pub source_component_name: String,
    pub source_property_path: Option<String>,
    pub target_entity: Option<Entity>,
    pub target_component_name: String,
    pub target_property_path: Option<String>,
    pub create_entity_func: Option<CreateEntityFunc>
}

impl ListBinding {
    pub fn to_string(&self) -> String {
        format!("<{}>.{}{} -> <{}>.{}{}", entity_to_string(&self.source_entity), self.source_component_name, path_to_string(&self.source_property_path), entity_to_string(&self.target_entity), self.target_component_name, path_to_string(&self.target_property_path))
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
        // 0: Query of bindable components that have recently changed
        Query<(Entity, All<&dyn Reactive>)>,
        Query<(Entity, Option<&mut Control>, Option<&mut TextLabel>, Option<&mut ImageRect>, Option<&mut Slider>, Option<&mut InputField>, Option<&mut BackgroundColor>, &AutoBindableProperty, Option<&mut AutoBindable>)>,
        Query<(Entity, &mut PropertyBinder)>,
        Bindings
        // 4: Query of all bindable records
        //Query<(Entity, &DBRecord, All<&dyn Reactive>)>
    )>,
    mut auto_bindable_list_query: Query<(Entity, &AutoBindableList, Option<&Children>)>,
    //form_query: Query<(Entity, &Form, Option<Changed<Form>>)>,
) {
    //let mut binding_queue = HashMap::<Entity, HashMap::<String, Box<dyn Reflect>>>::new();
    
    //let records = set.p3().iter().map(|(entity, record, bindables)| {
    //    (record, bindables.iter().next().unwrap())
    //});
    
    let mut changed_reactives = HashSet::new();

    for (entity, reactives) in set.p0().iter() {//bindable_struct_query.iter() {
        //binding_queue.insert(entity, HashMap::<String, Box<dyn Reflect>>::new());

        //commands.entity(entity).remove::<BindableChanged>();
        for reactive in reactives.iter_changed() {
            let component_name = reactive.into_inner().reflect_short_type_path().to_string();
            changed_reactives.insert((entity.clone(), component_name));
            //if was_changed.is_some_and(|x| x) {
            //if let Some(chat_view) = form.as_any().downcast_ref::<DetailedChatView>() {
            //}
     
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
            /*
            let reflect_ref = reactive.reflect_ref();
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
                    //let type_name = value.reflect_type_path();
                    let id = entity.to_bits().to_string();
                    //console::log!(format!("PROPOGATING VALUE OF TYPE: {type_name} FROM: {id}"));
                    let value = value.clone_value();
                    field_values.insert("".to_string(), value);
                }
            }
            */
        }
    }
    /* 
    let mut bindings = set.p3();
    for (entity, component_name) in changed_reactives.iter() {

        for binding_id in bindings.get_source_bindings(entity.clone()).clone() {
            let binding = bindings.get_binding(binding_id).into_inner();
            match binding {
                Binding::Path(binding) => {
                    if binding.source_component_name == component_name.to_string() {
                        info!("Applying path binding. Source type: {}", component_name);
        
                        bindings.apply_binding(binding_id);
                    }
                }
                Binding::List(binding) => {
                    if binding.source_component_name == component_name.to_string() {
                        info!("Applying list binding. Source type: {}", component_name);
        
                        bindings.apply_binding(binding_id);
                    }
                },
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
                                Bindable::set(bindable_struct.as_mut(), item.clone_value());
                                
                                info!("Set value for list element: {:?}", item);
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
   
    // TODO: Finish writing property binder
    // For reference: https://github.com/empathic-ai/altimit/blob/main/core/Core/Replication/PropertyBinder.cs
    for (property_entity, mut property_binder) in set.p2().iter_mut() {

        let mut field_values: Option<&HashMap<String, Box<dyn Reflect>>> = None;
        let mut index: Option<usize> = None;

        for property_entity in property_binder.property_entities.iter() {
            if let Some(i) = index {
                index = Some(i+1);
            } else {
                index = Some(0);
            }
            
            if let Some(property_entity) = property_entity {
                if let Some(_field_values) = binding_queue.get(&Entity::from_raw(property_entity.parse().unwrap())) {
                    field_values = Some(_field_values);
                    break;
                }
            } else {
                break;
            }
        }

        if let Some(i) = index {
            let property_index = i+1;
            let property_path_part = property_binder.property_path_parts[property_index].clone();
            if let Some(property_value) = field_values.unwrap().get(&property_path_part) {
                let mut new_entity: Option<String> = None;
                if let Some(value) = property_value.downcast_ref::<Entity>() {
                    new_entity = Some(value.to_bits().to_string());
                } else if let Some(value) = property_value.downcast_ref::<Option<Entity>>() {
                    if let Some(value) = value {
                        new_entity = Some(value.to_bits().to_string());
                    } else {
                        new_entity = None;
                    }
                }
                property_binder.property_entities[property_index] = new_entity;
            }
        }
    }

    // TODO: simply forward property value to bindable_struct. All bindable_property's will do this
    for (property_entity, mut control, mut label, mut image_rect, mut slider, mut input_field, mut background_color, bindable_property, bindable) in set.p1().iter_mut() {
        
        if let Some(entity) = bindable_property.entity {
            if let Some(field_values) = binding_queue.get(&entity) {
                if let Some(property_name) = bindable_property.property_path.as_ref() {
                    if let Some(property_value) = field_values.get(property_name) {
                        if bindable.is_some() {
                            let mut bindable = bindable.unwrap();
        
                            bindable.set(property_value.clone_value());
                            commands.queue(move|world: &mut World| {
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
                                    let property_name = bindable_property.property_path.clone();
                                    //console::log!(format!("SET BINDABLE VALUE: {property_name}"));
                                    //console::log!(format!("IS VISIBLE: {_value}"));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
         */
}

pub fn process_form_on_submit(
    mut commands: Commands,
    mut ev_reader: EventReader<SubmitEvent>,
    mut struct_query: Query<(Entity, All<&mut dyn Reactive>)>,
    input_query: Query<(&InputField, &AutoBindableProperty), (Changed<InputField>)>,
    mut form_query: Query<(Entity, Option<&OnSubmit>)>,
) {
    for (input_field, bindable_property) in input_query.iter() {
        if let Some(entity) = bindable_property.entity {
            if let Ok((entity, bindables)) = struct_query.get_mut(entity) {
                for mut bindable in bindables {
    
                    let reflect_ref = bindable.reflect_mut();
        
                    if let Some(property_path) = bindable_property.property_path.as_ref() {
                        if let ReflectMut::Struct(value) = reflect_ref {
                            if let Some(field) = value.field_mut(property_path) {
                                field.apply(input_field.text.clone().as_partial_reflect());
                            }
                        }
                    }
                }
            }
        }
    }

    for ev in ev_reader.read() {
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