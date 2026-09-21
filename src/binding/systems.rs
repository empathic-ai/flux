use std::any::{Any, TypeId};
use std::collections::{HashMap, HashSet};

use crate::prelude::*;

use bevy::ecs::error::HandleError;
use bevy::ecs::system::{SystemParam, SystemState, command};
use bevy::prelude::*;
use bevy::reflect::{
    DynamicEnum, ParsedPath, Reflect, ReflectFromPtr, ReflectMut, ReflectRef, Struct, TypeRegistry,
    Typed,
};
use bevy_trait_query::All;

use anyhow::{Result, anyhow};
use common::prelude::*;

pub type ReactivesQuery<'w, 's> = Query<'w, 's, (Entity, Option<&'static Name>, All<&'static mut dyn Reactive>)>;

#[derive(Event, Clone)]
pub struct OnChange {
    pub entity: Entity,
}


#[derive(SystemParam)]
pub struct FluxCommands<'w, 's> {
    commands: Commands<'w, 's>,
    pub db_config: ResMut<'w, DBConfig>,
}

impl FluxCommands<'_, '_> {
    pub fn load_record(&mut self, id: Id) -> Entity{
        load_record(id, &mut self.db_config, &mut self.commands)
    }
}

// TODO: Add check to see whether entity already exists or not
pub fn load_record(id: Id, db_config: &mut ResMut<DBConfig>, commands: &mut Commands) -> Entity{

    info!("Loading record with ID: {:#}", id);

    let entity = commands.spawn((DBRecord { id: id.clone() }, Loading {})).id();

    db_config.insert_entity(
        &id,
        entity
    );

            
    commands.send_network_event(Id::nil(), 
    TrackRecordEvent {
            entity_id: id,
        }
    );

    entity
}

#[derive(SystemParam)]
pub struct FluxWorld<'w, 's> {
    commands: Commands<'w, 's>,
    pub db_config: ResMut<'w, DBConfig>,
    // A list of all reactives in the world
    pub reactives: ReactivesQuery<'w, 's>
}

impl<'w, 's> FluxWorld<'w, 's> {
    pub fn load_record(&mut self, id: Id) -> Entity {
        load_record(id, &mut self.db_config, &mut self.commands)
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
#[cfg(feature = "bevy_std")]
pub fn propogate_forms(
    mut commands: Commands,
    //type_registry: Res<AppTypeRegistry>,
    //mut ev_reader: EventReader<SubmitEvent>,
    //mut label_query: Query<(&mut Label, &AutoBindableProperty)>,
    mut set: ParamSet<(
        // 0: Query of bindable components that have recently changed
        Query<(Entity, All<&dyn Reactive>)>,
        Query<(
            Entity,
            Option<&mut Control>,
            Option<&mut TextLabel>,
            Option<&mut ImageRect>,
            Option<&mut Slider>,
            Option<&mut InputField>,
            Option<&mut BackgroundColor>,
            &AutoBindableProperty,
            Option<&mut AutoBindable>,
        )>,
        Query<(Entity, &mut PropertyBinder)>,
        FluxWorld, // 4: Query of all bindable records
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

    for (entity, reactives) in set.p0().iter() {
        //bindable_struct_query.iter() {
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

/// Resolves a (entity, component_name, property_path) triple all the way down to a final
/// value, following entity jumps (Id -> Entity -> component) via PathWalker as it walks
/// the path. Returns None if the root component is missing, the path fails to parse, or
/// the walk stops early (dead entity, missing component, Option::None, bad access, etc.).
pub fn resolve_source_value<'w, 's>(
    reactives: &ReactivesQuery<'w, 's>,
    db_config: &DBConfig,
    entity: Entity,
    component_name: String,
    property_path: Option<String>,
) -> Option<(Option<String>, Box<dyn PartialReflect>)> {
    let resolver = ReactiveResolver { reactives, db_config };


    let (root_name, root_value) = match resolver.get_reactive(entity, &component_name) {
        Some((name, root)) => (name, root),
        None => {
            //info!("Failed to get reactive component.");
            return None;
        }
    };

    let Some(property_path) = property_path else {
        return Some((root_name, root_value));
    };

    let parsed_path = OptionalParsedPath::parse(&property_path).expect("Failed to parse property path");
    let mut walker = PathWalker::new(root_value, &parsed_path, &resolver);

    // Drain the walker fully; PathStep values themselves aren't needed here,
    // only the final resting value and whether it stopped early.
    for _step in &mut walker {

    }

    if let Some(stop_reason) = walker.stop_reason() {
        match stop_reason {
            PathWalkStop::EntityMissing => {
                //info!("Entity missing! Property path: {}", property_path)
            },
            PathWalkStop::ComponentMissing(component_name) => {
                //info!("Component missing! Component: {}, Property path: {}", component_name, property_path)
            },
            PathWalkStop::OptionWasNone => {
                //info!("Option was None! Property path: {}", property_path)
            },
            PathWalkStop::AccessError => {
                //info!("Access error! Property path: {}", property_path)
            }
        }
        return None;
    }

    Some((root_name, walker.current_value().clone_value()))
}

/// Resolves `target_property_path` (or the whole component, if `None`) starting from
/// `target_entity`'s `target_component_name`, following entity jumps through Id-typed
/// fields exactly like the source-side walker — but chaining real mutable borrows within
/// each component, so the final leaf can actually be written to (not cloned).
pub fn apply_value_at_target_path<'w, 's>(
    reactives: &mut ReactivesQuery<'w, 's>,
    db_config: &DBConfig,
    target_entity: Entity,
    target_component_name: String,
    target_property_path: Option<String>,
    mut source_value: Box<dyn PartialReflect>,
    changed_reactives: &mut HashSet<(Entity, String)>,
) -> Result<()> {
    let parsed_path = match &target_property_path {
        Some(p) => OptionalParsedPath::parse(p)
            .map_err(|e| anyhow!("Failed to parse target property path '{}': {}", p, e))?,
        None => OptionalParsedPath(Vec::new()),
    };

    let mut current_entity = target_entity;
    let mut current_component_name = target_component_name.clone();
    let mut path_index = 0usize;

    loop {
        let Ok((_, _, mut target_reactives)) = reactives.get_mut(current_entity) else {
            return Err(anyhow!(
                "Failed to find entity <{}> while walking target path.",
                current_entity
            ));
        };

        let Some(mut target_bindable) = target_reactives
            .iter_mut()
            .find(|x| x.reflect_short_type_path() == current_component_name)
        else {
            return Err(anyhow!(
                "Failed to find component '{}' on entity <{}> while walking target path.",
                current_component_name,
                current_entity
            ));
        };

        let mut current_value: &mut dyn PartialReflect = target_bindable.as_partial_reflect_mut();
        let mut jumped = false;

        // Walk forward, chaining real mutable borrows, until the path ends or we hit
        // an Id-typed value that needs to jump to a different entity/component.
        while path_index < parsed_path.0.len() {
            // A value slot is transparent while traversing into its contents.
            // Leave the wrapper intact when the path ends at the slot itself.
            while current_value.try_downcast_ref::<Dynamic>().is_some() {
                current_value = current_value.try_downcast_mut::<Dynamic>().unwrap().as_mut();
            }
            // Match PathWalker: unwrap an Option before interpreting the next
            // access, especially before checking for an Id entity jump.
            if is_option(&current_value.to_dynamic()) {
                let ReflectMut::Enum(value) = current_value.reflect_mut() else {
                    return Err(anyhow!("Expected Option enum while walking target path"));
                };
                current_value = value.field_at_mut(0)
                    .ok_or_else(|| anyhow!("Option was None while walking target path"))?;
            }
            if let Some(id) = Id::from_reflect(&*current_value) {
                let offset_access = &parsed_path.0[path_index];
                let component_name = offset_access.access.display_value().to_string();

                let Some(next_entity) = db_config.get_entity(&id) else {
                    return Err(anyhow!(
                        "Failed to resolve Id '{:#}' to an entity while walking target path.",
                        id
                    ));
                };

                current_entity = next_entity;
                current_component_name = component_name;
                path_index += 1;
                jumped = true;
                break; // drop this component's mutable borrow; outer loop refetches fresh
            }

            let offset_access = &parsed_path.0[path_index];
            current_value = offset_access
                .access
                .element_mut(current_value, offset_access.offset)
                .map_err(|err| {
                    anyhow!(
                        "Failed to access field '{}' while walking target path: {}",
                        offset_access.access,
                        err
                    )
                })?;
            path_index += 1;
        }

        if jumped {
            continue;
        }

        // Path fully walked — `current_value` is a real mutable reference to the leaf.
        return apply_value_with_changes(
            current_value,
            source_value,
            current_entity,
            current_component_name,
            changed_reactives,
        );
    }
}


pub fn apply_value<TTarget, TSource>(
    target_value: &mut TTarget,
    source_value: TSource) -> Result<()>
    where TTarget: PartialReflect, TSource: PartialReflect {
    let mut changed_reactives = HashSet::<(Entity, String)>::default();
    apply_value_with_changes(target_value, source_value.to_dynamic(), Entity::PLACEHOLDER, String::new(), &mut changed_reactives)
}

/// Shared write logic — identical semantics to your original is_dynamic/is_option handling.
fn apply_value_with_changes(
    target_value: &mut dyn PartialReflect,
    source_value: Box<dyn PartialReflect>,
    target_entity: Entity,
    target_component_name: String,
    changed_reactives: &mut HashSet<(Entity, String)>,
) -> Result<()> {

    // A Dynamic field is an assignable value slot, including whole Options
    // and collections. Handle it before shape coercion and list patching.
    let source_value = Dynamic::unwrap(source_value);
    if let Some(target_value) = target_value.try_downcast_mut::<Dynamic>() {
        if target_value.as_ref().reflect_partial_eq(source_value.as_ref()) != Some(true) {
            *target_value = Dynamic(source_value);
            changed_reactives.insert((target_entity, target_component_name));
        }
        return Ok(());
    }

    let mut _source_value: Box<dyn PartialReflect> = source_value.to_dynamic();

    // Resolve Option<T> <-> T mismatches BEFORE the is_dynamic conversion below, since
    // both `apply` (kind must match) and `ReflectFromReflect::from_reflect` (shape must
    // match) need the source already massaged into the target's kind.
    let (source_is_option, source_inner_value) = get_inner_if_option(&_source_value);
    let (target_is_option, _) = get_inner_if_option(&target_value.to_dynamic());

    let different_types =
        _source_value.reflect_short_type_path() != target_value.reflect_short_type_path();

    tracing::trace!(?target_entity, %target_component_name, source_is_option, target_is_option, different_types, "Preparing binding value");

    if different_types {
        if source_is_option && !target_is_option {
            // Some(concrete_T)/None source -> bare T (or dynamic_T) target: unwrap,
            // or bail if None since there's nothing to apply.
            match source_inner_value {
                Some(inner_value) => _source_value = inner_value,
                None => return Ok(()),
            }
        } else if target_is_option && !source_is_option {
            // bare T (or dynamic_T) source -> Option<T> target: wrap as Some(source)
            // shaped like the target's enum type, so kinds match for `apply`/`set`.
            _source_value = wrap_as_option(target_value, _source_value)?;
        }
    }

    // Dynamic map slots assign complete snapshots, not reflection patches.
    // DynamicMap is PartialReflect-only, so inspect its kind rather than downcasting.
    if target_value.is_dynamic() && matches!(target_value.reflect_ref(), ReflectRef::Map(_)) {
        let ReflectRef::Map(source) = _source_value.reflect_ref() else {
            return Err(anyhow!("Binding target type mismatch: expected a map"));
        };
        if target_value.reflect_partial_eq(_source_value.as_ref()) != Some(true) {
            let ReflectMut::Map(target) = target_value.reflect_mut() else { unreachable!() };
            // The pinned Bevy fork's DynamicMap::drain leaves its key index
            // populated. Remove entries through Map to keep that index valid.
            let keys: Vec<_> = target.iter().map(|(key, _)| key.clone_value()).collect();
            for key in keys { target.remove(key.as_ref()); }
            for (key, value) in source.iter() {
                target.insert_boxed(key.clone_value(), value.clone_value());
            }
            changed_reactives.insert((target_entity, target_component_name));
        }
        return Ok(());
    }

    // Handle list/array targets explicitly, before the dynamic-conversion path,
    // since ReflectFromReflect isn't meant for Dynamic* wrapper types.
    if matches!(target_value.reflect_ref(), ReflectRef::List(_) | ReflectRef::Array(_)) {
        if !target_value
            .reflect_partial_eq(_source_value.as_partial_reflect())
            .unwrap_or(false)
        {
            target_value
                .try_apply(_source_value.as_partial_reflect())
                .map_err(|err| anyhow!(
                    "Failed to apply list/array value to target component '{}': {}",
                    target_component_name, err
                ))?;
            // Reflection applies lists as a patch; bindings assign snapshots.
            // Remove trailing items when a filtered/merged list becomes shorter.
            if let (ReflectMut::List(target), ReflectRef::List(source)) =
                (target_value.reflect_mut(), _source_value.reflect_ref())
            {
                while target.len() > source.len() { target.pop(); }
            }
            changed_reactives.insert((target_entity, target_component_name));
        }
        return Ok(());
    }

    if target_value.is_dynamic() {

        let _source_value = _source_value.to_dynamic();

        if !target_value
            .reflect_partial_eq(_source_value.as_partial_reflect())
            .unwrap_or(false)
        {
            tracing::trace!(?target_entity, %target_component_name, "Applying dynamic binding value");
            target_value.try_apply(_source_value.as_ref()).map_err(|error| anyhow!("Binding target type mismatch: {error}"))?;
            changed_reactives.insert((target_entity, target_component_name));
        }
        /* 
        let target_type_name = target_value.reflect_short_type_path();

        let mut type_registry = TypeRegistry::new();
        type_registry.register_global_types();

        let type_registration = type_registry
            .get_with_short_type_path(target_type_name)
            .ok_or_else(|| anyhow!("Failed to find type registration for '{}'", target_type_name))?;

        let reflect_from_reflect: &ReflectFromReflect = type_registration
            .data::<ReflectFromReflect>()
            .expect("`ReflectFromReflect` should be registered");

        let converted = reflect_from_reflect
            .from_reflect(_source_value.as_ref())
            .ok_or_else(|| anyhow!("Failed to convert source value to '{}'", target_type_name))?;

        info!("Converted value: {}. Target value: {}", converted.as_partial_reflect().to_string_pretty(), target_value.as_partial_reflect().to_string_pretty());
        
        if !target_value
            .reflect_partial_eq(converted.as_partial_reflect())
            .unwrap_or(false)
        {
            info!(
                "Applying (dynamic) value to <{}>.{}. Target type: {}. Converted value: {:?}",
                target_entity, target_component_name, target_type_name, converted.as_partial_reflect().to_string_pretty()
            );

            target_value.try_as_reflect_mut().unwrap().set(converted);
            changed_reactives.insert((target_entity, target_component_name));
        }
        */
    } else {
        tracing::trace!("Applying concrete binding value");
        if !target_value
            .reflect_partial_eq(_source_value.as_partial_reflect())
            .unwrap_or(false)
        {
            target_value.try_apply(_source_value.as_ref()).map_err(|error| anyhow!("Binding target type mismatch: {error}"))?;
            changed_reactives.insert((target_entity, target_component_name));
        }
    }

    Ok(())
}


/// Wraps a bare source value as `Some(source_value)`, producing a `DynamicEnum` shaped
/// like the target's `Option<T>` type so `.apply()` can write into it correctly.
fn wrap_as_option(
    target_value: &dyn PartialReflect,
    source_value: Box<dyn PartialReflect>,
) -> Result<Box<dyn PartialReflect>> {
    use bevy::reflect::{DynamicTuple, DynamicVariant};

    let ReflectRef::Enum(target_enum) = target_value.reflect_ref() else {
        return Err(anyhow!(
            "Expected target to be an Option (enum) when wrapping source value, but got '{}'",
            target_value.reflect_short_type_path()
        ));
    };

    let type_path = target_enum
        .get_represented_type_info()
        .map(|info| info.type_path().to_string())
        .unwrap_or_else(|| target_value.reflect_short_type_path().to_string());

    let mut tuple = DynamicTuple::default();
    tuple.insert_boxed(source_value);

    let mut dynamic_enum = DynamicEnum::new("Some", DynamicVariant::Tuple(tuple));
    dynamic_enum.set_represented_type(target_enum.get_represented_type_info());

    Ok(Box::new(dynamic_enum))
}

#[cfg(feature = "bevy_std")]
pub fn process_form_on_submit(
    mut commands: Commands,
    mut ev_reader: EventReader<SubmitEvent>,
    mut struct_query: Query<(Entity, All<&mut dyn Reactive>), Without<InputField>>,
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
        commands.entity(ev.0).insert(Submitted {});
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
