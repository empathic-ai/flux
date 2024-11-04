use std::any::TypeId;

use crate::prelude::*;

use bevy::prelude::*;
use bevy::reflect::TypeInfo::Struct;
use bevy::reflect::{TypeRegistry, ReflectMut, ReflectRef};
use bevy::utils::HashMap;
use bevy_trait_query::All;

use common::prelude::*;

#[derive(Resource)]
pub struct ReferenceChanges {
    pub changes: HashMap<TypeId, Vec<(Entity, String, String)>>
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
        Query<(Entity, All<&dyn Bindable>), Or<(With<BindableChanged>, Changed<AutoBindable>)>>,
        Query<(Entity, Option<&mut Control>, Option<&mut BLabel>, Option<&mut ImageRect>, Option<&mut Slider>, Option<&mut InputField>, Option<&mut BackgroundColor>, &AutoBindableProperty, Option<&mut AutoBindable>)>,
        Query<(Entity, &mut PropertyBinder)>,
        // 4: Query of all bindable records
        Query<(Entity, &DBRecord, All<&dyn Bindable>)>
    )>,
    mut auto_bindable_list_query: Query<(Entity, &AutoBindableList, Option<&Children>)>,
    //form_query: Query<(Entity, &Form, Option<Changed<Form>>)>,
) {
    let mut binding_queue = HashMap::<Entity, HashMap::<String, Box<dyn Reflect>>>::new();
    
    let records = set.p3().iter().map(|(entity, record, bindables)| {
        (record, bindables.iter().next().unwrap().get())
    });
    
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
                    //let type_name = value.reflect_type_path();
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
}



pub fn process_form_on_submit(
    mut commands: Commands,
    mut ev_reader: EventReader<SubmitEvent>,
    mut struct_query: Query<(Entity, All<&mut dyn Bindable>)>,
    input_query: Query<(&InputField, &AutoBindableProperty), (Changed<InputField>)>,
    mut form_query: Query<(Entity, Option<&OnSubmit>)>,
) {
    for (input_field, bindable_property) in input_query.iter() {
        if let Some(entity) = bindable_property.entity {
            if let Ok((entity, bindables)) = struct_query.get_mut(entity) {
                for mut bindable in bindables {
    
                    let mut reflect = bindable.get();
                    let reflect_ref = reflect.reflect_mut();
        
                    if let Some(property_path) = bindable_property.property_path.as_ref() {
                        if let ReflectMut::Struct(value) = reflect_ref {
                            if let Some(field) = value.field_mut(property_path) {
                                field.set(Box::new(input_field.text.clone()));
                            }
                        }
                    }
                    bindable.set(reflect);
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