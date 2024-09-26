use bevy::{ecs::system::EntityCommands, prelude::*, utils::default};
use bevy_cobweb_ui::prelude::*;
use bevy_cobweb::prelude::*;

use crate::prelude::*;
use common::prelude::*;

pub struct EntityBuilder<'a> {
    entity_commands: EntityCommands<'a>,
    //custom_steps: Vec<Box<dyn Fn(&mut EntityCommands) + 'a>>, // Store closures for custom steps
}

impl<'a> EntityBuilder<'a> {
    pub fn new(parent: EntityCommands<'a>) -> Self {
        Self { 
            entity_commands: parent, 
            //custom_steps: Vec::new(),
        }
    }

    pub fn from(parent: &'a mut bevy::prelude::ChildBuilder<'_>) -> Self {
        let entity_commands: EntityCommands<'_> = parent.spawn_empty();
        
        Self {
            entity_commands: entity_commands, 
            //custom_steps: Vec::new(),
        }
    }
}

impl<'a>  Builder<'a> for EntityBuilder<'a> {
    fn get_commands(&mut self) -> &mut EntityCommands<'a> {
        &mut self.entity_commands
    }
}

impl<'a> UiReactEntityCommandsExt for EntityBuilder<'a> {
    fn insert_reactive<T: ReactComponent>(&mut self, component: T) -> &mut Self {
        self.get_commands().insert_reactive(component);
        self
    }
    
    fn insert_derived<T: ApplyLoadable>(&mut self, value: T) -> &mut Self {
        todo!()
    }
    
    fn on_event<T: Send + Sync + 'static>(&mut self) -> OnEventExt<'_, T> {
        todo!()
    }
    
    fn despawn_on_event<T: Send + Sync + 'static>(&mut self) -> &mut Self {
        todo!()
    }
    
    fn despawn_on_broadcast<T: Send + Sync + 'static>(&mut self) -> &mut Self {
        todo!()
    }
    
    fn update_on<M, C, T, R>(&mut self, triggers: T, reactor: R) -> &mut Self
    where
        C: IntoSystem<(), (), M> + Send + Sync + 'static,
        T: ReactionTriggerBundle,
        R: FnOnce(Entity) -> C {
        self.get_commands().update_on(triggers, reactor);
        self
    }
}

pub trait BaseBuilder<'a>: Builder<'a> + UiReactEntityCommandsExt {
    fn dynamic_view(&mut self, prompt: String) -> &mut Self {
        self.insert(DynamicView { prompt: prompt })
    }
    
    fn stylized_image(&mut self, is_horizontal: bool, color: Color, image: &str) -> &mut Self {
        if is_horizontal {
            self.get_commands().insert(WidthLessThan { is_visible: false, width: 600.0 });
        } else {
            self.get_commands().insert(HideOnHeightLessThan(800.0));
        }
        self.insert((
            Control {
                is_visible: true,
                //ExpandHeight: true,
                fixed_height: 300.0,
                fixed_width: 300.0,
                BorderRadius: if is_horizontal {
                    Vec4::new(SMALL_SPACE, SMALL_SPACE, 0.0, 0.0)
                } else {
                    Vec4::new(0.0, SMALL_SPACE, SMALL_SPACE, 0.0)
                },
                Padding: Vec4::splat(10.0),
                ..default()
            },
            Container { ..default() },
            VList {
                spacing: SMALL_SPACE,
                ..default()
            },
            BackgroundColor(color),
        ))
        .with_children(|parent| {
            parent.spawn((
                Control {
                    ExpandWidth: true,
                    ExpandHeight: true,
                    //FixedWidth: 285.0,
                    //fixed_height: 285.0,
                    BorderRadius: Vec4::splat(5.0),
                    ..default()
                },
                ImageRect {
                    image: image.to_string(),
                    ..default()
                },
            ))
            .id();
        })
    }

    fn on_click_event<E: Event + std::clone::Clone>(&mut self, event: E) -> &mut Self {
        
        self.on_click(|entity| {
            move |mut commands: Commands| {
            //move |command| {
                let event = event.clone();
                commands.add(move |world: &mut World| {
                    log("Sending click event!");
                    world.send_event(event);
                });
            }
        })
    }

    fn by_empathic_title(&mut self, brightness: f32, size: f32) -> &mut Self {
        self.expand_width().h_list().padding(Vec4::splat(HALF_SMALL_SPACE*size)).with_children(|parent| {
            //parent.child().label("by".to_string(), DEFAULT_FONT_SIZE*size, Color::rgb(brightness, brightness, brightness), Anchor::MiddleLeft, true);
            //parent.child().fixed_width(7.5*size);
            parent.child().insert((
                ImageRect {
                    image: "assets/icons/Empathic Title.webp".to_string(),
                    brightness: brightness,
                    ..default()
                },
            )).fixed_width(120.0*size).fixed_height(DEFAULT_FONT_SIZE*size);//.expand_height();
        })
    }
/*
    fn update_on<M, C, T, R>(&mut self, triggers: T, reactor: R) -> &mut Self
    where
        C: IntoSystem<(), (), M> + Send + Sync + 'static,
        T: ReactionTriggerBundle,
        R: FnOnce(Entity) -> C {
        self.get_commands().update_on(triggers, reactor);
        self
    } */

    fn on_click<M, C, R>(&mut self, on_click: R) -> &mut Self 
    where
        C: IntoSystem<(), (), M> + Send + Sync + 'static,
        R: FnOnce(Entity) -> C, {

        let id = self.id();
        let callback = (on_click)(id);

        let syscommand = self.get_commands().commands().spawn_system_command(callback);
        self.upsert(|comp: &mut Button|{}).insert(
            OnClick {
                func: syscommand
            }
        )
    }
    
    fn on_submit(&mut self, on_submit: SubmitFunc) -> &mut Self {
        self.insert(
            OnSubmit {
                func: on_submit
            }
        )
    }

    fn bind<T: Default + Reflect>(&mut self) -> &mut Self {
        self.insert(
            AutoBindable {
                value: Box::<T>::new(Default::default())
            }
        )
    }

    fn bind_value<T: Default + Reflect>(&mut self, value: T) -> &mut Self {
        self.insert((
            AutoBindable {
                value: Box::<T>::new(value)
            },
            BindableChanged {}
        ))
    }

    fn bind_property_with_func(&mut self, entity: Entity, property_name: &str, entity_func: SetPropertyFunc) -> &mut Self {
        self.insert(
            AutoBindableProperty {
                entity: entity,
                component_name: "*".to_string(),
                property_name: property_name.to_string(),
                entity_func: Some(entity_func)
            }
        )
    }

    fn panel_dark_image_button(&mut self, image: String) -> &mut Self {
        self.panel()
        .with_children(|parent| {
            parent.child().dark_image_button(image, None);
        })
    }

    fn panel(&mut self) -> &mut Self {
        self.insert((
            Control {
                Padding: Vec4::splat(SMALL_SPACE),
                BorderRadius: Vec4::splat(SMALL_SPACE),
                is_visible: true,
                ..default()
            },
            Container { ..default() },
            HList {
                spacing: SMALL_SPACE,
                ..default()
            },
            Shadow {},
            BackgroundColor(Color::WHITE),
        ))
    }

    fn bind_component_property(&mut self, entity: Entity, component_name: &str, property_name: &str) -> &mut Self {
        self.insert(
            AutoBindableProperty {
                entity: entity,
                component_name: component_name.to_string(),
                property_name: property_name.to_string(),
                entity_func: None
            }
        )
    }

    fn bind_property(&mut self, entity: Entity, property_name: &str) -> &mut Self {
        self.insert(
            AutoBindableProperty {
                entity: entity,
                component_name: "*".to_string(),
                property_name: property_name.to_string(),
                entity_func: None
            }
        )
    }

    fn bind_path(&mut self, path: Vec<&str>, entity_func: SetPropertyFunc) -> &mut Self {
        self.insert(
            PropertyBinder {
                property_path_parts: path.iter().map(|x| x.to_string()).collect(),
                property_entities: vec![],
                entity_func: Some(entity_func)
            }
        )
    }

    fn bind_path_list(&mut self, path: Vec<&str>, create_entity_func: CreateEntityFunc) -> &mut Self {
        self.insert(
            PropertyBinder {
                property_path_parts: path.iter().map(|x| x.to_string()).collect(),
                property_entities: vec![],
                entity_func: None
            }
        )
    }

    fn bind_component_list(&mut self, source_entity: Entity, component_name: &str, property_name: &str, create_entity_func: CreateEntityFunc) -> &mut Self {
        self.insert(
            AutoBindableList {
                entity: source_entity,
                property_name: property_name.to_string(),
                create_entity: Some(create_entity_func)
            }
        )
    }

    fn bind_list(&mut self, source_entity: Entity, property_name: &str, create_entity_func: CreateEntityFunc) -> &mut Self {
        self.insert(
            AutoBindableList {
                entity: source_entity,
                property_name: property_name.to_string(),
                create_entity: Some(create_entity_func)
            }
        )
    }
/* 
    pub fn with_children_builder<F>(mut self, f: F) -> Self
    where
        F: FnOnce(&'a mut EntityChildBuilder<'w, 's, '_>),
    {
        self.entity_commands.with_children(|parent: &'a mut ChildBuilder<'w, 's, '_>| {
            let mut child_builder: EntityChildBuilder<'w, 's, '_> = EntityChildBuilder::new(parent);
            f(&mut child_builder);
        });

        self
    }
*/

    fn router(&mut self) -> &mut Self {
        self.insert(
            Router { ..default() }
        )
    }

    fn route(&mut self, name: &str) -> &mut Self {
        self.insert(
            Route { name: name.to_string() }
        )
    }

    fn large_space(&mut self, image: String) -> &mut Self {
        self.insert((
            Control {
                fixed_width: MEDIUM_LARGE,
                fixed_height: MEDIUM_LARGE,
                BorderRadius: Vec4::splat(10.0),
                ..default()
            },
            ImageRect { image, ..default() },
            Button { ..default() },
            InteractState { ..default() }
            //Shadow {}
        )).rounded().scale_on_hover().on_click(|entity| {
            |mut commands: Commands| {
            }
        }
            //CommandFunc::new(move |commands: &mut Commands| {
                //bevy_web::set_route("lobby".to_string());
            //})
        )
    }

    fn scale_on_hover(&mut self) -> &mut Self {
        let entity = self.id();
        self.upsert(|comp: &mut Button| {}).upsert(|comp: &mut InteractState| {}).bind_property_with_func(entity, "is_hovering",
        SetPropertyFunc::new(move|commands, _entity, reflect| {
            if let Ok(value) = reflect.downcast::<bool>() {
                commands.entity(entity).builder().upsert(move |comp: &mut Control| {
                    comp.Scale = if *value { 1.005 } else { 0.995 }
                });
            }
        }))
    }

    // TODO: Rework to select multiple objects in a list and run actions on them
    // CSS: outline: darkorange, ouline-width: 4px, outline-style: solid 

    fn selectable(&mut self) -> &mut Self {
        let entity = self.id();
        self.upsert(|comp: &mut Button| {}).upsert(|comp: &mut InteractState| {}).bind_property_with_func(entity, "is_clicking",
        SetPropertyFunc::new(move|commands, _entity, reflect| {
            if let Ok(value) = reflect.downcast::<bool>() {
                commands.entity(entity).builder().upsert(move |comp: &mut Control| {
                    comp.Scale = if *value { 1.005 } else { 0.995 }
                });
            }
        }))
    }

    fn mini_group(&mut self) -> &mut Self {
        self.insert((
            Control {
                fixed_width: MEDIUM_LARGE,
                fixed_height: MEDIUM_LARGE,
                BorderRadius: Vec4::splat(10.0),
                ..default()
            },
            Container {},
            HList {
                wrapped: true,
                ..default()
            },
            Shadow {},
            Button { ..default() },
        ))
        .with_children(|parent| {
            parent.child().mini_group_avatar_image(
                "assets/avatars/Taby/Default.png".to_string(),
                Vec4::new(0.0, 10.0, 0.0, 0.0),
            );
            parent.child().mini_group_avatar_image(
                "assets/avatars/Taby/Chef.png".to_string(),
                Vec4::new(0.0, 0.0, 10.0, 0.0),
            );
            parent.child().mini_group_avatar_image(
                "assets/avatars/Taby/Chef.png".to_string(),
                Vec4::new(10.0, 0.0, 0.0, 0.0),
            );
            parent.child().mini_group_avatar_image(
                "assets/avatars/Taby/Wizard.png".to_string(),
                Vec4::new(0.0, 0.0, 0.0, 10.0),
            );
        })
    }

    fn mini_group_avatar_image(&mut self, image: String, border_radius: Vec4) -> &mut Self {
        self.insert((
            Control {
                fixed_width: MEDIUM_LARGE / 2.0,
                fixed_height: MEDIUM_LARGE / 2.0,
                BorderRadius: border_radius,
                ..default()
            },
            ImageRect { image, ..default() },
        ))
    }

    fn mini_image(&mut self, image: String) -> &mut Self {
        self.image(image).fixed_size(MEDIUM_LARGE / 2.0)
    }

    fn medium_image(&mut self, image: String) -> &mut Self {
        self.image(image).fixed_size(MEDIUM_LARGE / 1.5)
    }

    fn large_image(&mut self, image: String) -> &mut Self {
        self.image(image).fixed_size(MEDIUM_LARGE)
    }

    fn image(&mut self, image: String) -> &mut Self {
        self.insert((
            ImageRect { image, ..default() },
        ))
    }

    fn entity_with_children<F>(&mut self, f: F) -> &mut Self
    where
        F: FnOnce(Entity, &mut ChildBuilder<'_>),
    {
        let entity = self.get_commands().id();
        self.get_commands().with_children(|parent: &mut ChildBuilder<'_>| {
            f(entity, parent);
        });
        self
    }

    fn with_children<F>(&mut self, f: F) -> &mut Self
    where
        F: FnOnce(&mut ChildBuilder<'_>),
    {
        self.get_commands().with_children(f);
        self
    }

    fn stone_slice(&mut self) -> &mut Self {
        self.insert(ImageRect {
            // StoneButton or GoldButton
            image: "assets/icons/GoldButton.png".to_string(),
            is_nine_slice: true,
            border_image_slice: Vec4::splat(60.0),
            border_image_width: Vec4::splat(15.0),
            ..default()
        })
    }

    fn gem_slice(&mut self) -> &mut Self {
        self.insert(
            ImageRect {
                image: "assets/icons/GemButton2.png".to_string(),
                is_nine_slice: true,
                border_image_slice: Vec4::splat(150.0),
                border_image_width: Vec4::splat(20.0),
                ..default()
            }
        )
    }

    fn colored_button(&mut self, label: String, color: Color) -> &mut Self {
        self.insert((
            Control {
                BorderRadius: Vec4::splat(10.0),
                ExpandWidth: true,
                Padding: Vec4::splat(15.0),
                ..default()
            },
            Button { ..default() },
            Container {},
            VList { ..default() },
            Shadow {},
            BackgroundColor(color),
        )).with_children(|parent| {
            parent.spawn((
                Control {
                    ExpandWidth: true,
                    ..default()
                },
                BLabel {
                    alignment: Anchor::MiddleCenter,
                    text: label,
                    is_single_line: true,
                    color: Color::WHITE,
                    ..default()
                },
            ));
        }).scale_on_hover()
    }
    
    fn h_list(&mut self) -> &mut Self {
        self.upsert(move |comp: &mut Container| {}).upsert(move |comp: &mut HList| {
            
        })
    }

    fn font_size(&mut self, size: f32) -> &mut Self {
        self.upsert(move |label: &mut BLabel| {
            label.font_size = size;
        })
    }

    fn is_single_line(&mut self) -> &mut Self {
        self.upsert(move |label: &mut BLabel| {
            label.is_single_line = true;
        })
    }

    fn stretch_for_list(&mut self, stretch: bool) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.stretch = stretch;
        })
    }

    fn h_wrapped(&mut self, wrapped: bool) -> &mut Self {
        self.upsert(move |comp: &mut HList| {
            comp.wrapped = wrapped;
        })
    }

    fn h_anchor(&mut self, anchor: Anchor) -> &mut Self {
        self.upsert(move |comp: &mut HList| {
            comp.anchor = anchor;
        })
    }

    fn v_anchor(&mut self, anchor: Anchor) -> &mut Self {
        self.upsert(move |comp: &mut VList| {
            comp.anchor = anchor;
        })
    }

    fn v_list(&mut self) -> &mut Self {
        self.upsert(move |comp: &mut Container| {}).upsert(move |comp: &mut VList| {})
    }

    fn background_color(&mut self, c: Color) -> &mut Self {
        self.upsert(move |comp: &mut BackgroundColor| comp.0 = c)
    }

    fn small_padding(&mut self) -> &mut Self {
        self.padding(Vec4::splat(SMALL_SPACE))
    }

    fn padding(&mut self, padding: Vec4) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.Padding = padding;
        })
    }

    fn form(&mut self) -> &mut Self {
        self.insert(
            Form { ..default() }
        )
    }

    fn input_field(&mut self, placeholder: String, input_type: InputType) -> &mut Self {
        self.insert((
            Control {
                ExpandWidth: true,
                BorderRadius: Vec4::splat(10.0),
                is_visible: true,
                Padding: Vec4::splat(SMALL_SPACE),
                ..default()
            },
            Container { ..default() },
            HList { ..default() },
            Shadow {},
            InputField {
                placeholder: placeholder.to_string(),
                input_type: input_type,
                ..default()
            },
            BackgroundColor(Color::WHITE),
        ))
    }

    fn input_area(&mut self, placeholder: String, input_type: InputType) -> &mut Self {
        self.insert((
            Control {
                ExpandWidth: true,
                BorderRadius: Vec4::splat(10.0),
                is_visible: true,
                Padding: Vec4::splat(SMALL_SPACE),
                ..default()
            },
            Container { ..default() },
            HList { ..default() },
            Shadow {},
            InputField {
                placeholder: placeholder.to_string(),
                input_type: input_type,
                alignment: Anchor::UpperLeft,
                ..default()
            },
            BackgroundColor(Color::WHITE),
        ))
    }

    fn insert(&mut self, bundle: impl Bundle) -> &mut Self {
        self.get_commands().insert(bundle);
        self
    }

    fn labeled_line(&mut self, text: String) -> &mut Self {
        self.insert((
            Control {
                ExpandWidth: true,
                ..default()
            },
            Container { ..default() },
            HList {
                spacing: SMALL_SPACE,
                anchor: Anchor::MiddleCenter,
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.child().flexible_h_line();
            parent.child().label(text, 13.0, Srgba::gray(0.5).into(), Anchor::MiddleCenter, true);
            parent.child().flexible_h_line();
        })
    }

    fn label(&mut self, text: String, font_size: f32, color: Color, alignment: Anchor, is_single_line: bool) -> &mut Self {
        self.upsert(move |comp: &mut BLabel| {
            comp.alignment = alignment;
            comp.text = text;
            comp.is_single_line = is_single_line;
            comp.color = color;
            comp.font_size = font_size;
        })
    }

    fn flexible_h_line(&mut self) -> &mut Self {
        self.insert((
            Control {
                ExpandWidth: true,
                fixed_height: 1.0,
                BorderRadius: Vec4::splat(1.0),
                ..default()
            },
            BackgroundColor(Color::rgb(0.8, 0.8, 0.8)),
        ))
    }

    fn flexible_v_line(&mut self) -> &mut Self {
        self.insert((
            Control {
                ExpandHeight: true,
                fixed_width: 1.0,
                BorderRadius: Vec4::splat(1.0),
                ..default()
            },
            BackgroundColor(Color::rgb(0.8, 0.8, 0.8)),
        ))
    }


    fn show_width_less_than(&mut self, width: f32) -> &mut Self {
        self.insert(
            WidthLessThan {
                is_visible: true,
                width: width
            }
        )
    }    

    fn hide_width_less_than(&mut self, width: f32) -> &mut Self {
        self.insert(
            WidthLessThan {
                is_visible: false,
                width: width
            }
        )
    }    

    fn upsert<T, F>(&mut self, f: F) -> &mut Self where F: FnOnce(&mut T) + Send + 'static, T: Default + Component {
        self.get_commands().add(move |entity: Entity, world: &mut World| {
            let mut comp = world.get_mut::<T>(entity);
            if comp.is_none() {
                comp = None;
                if let Some(mut entity_mut) = world.get_entity_mut(entity) {
                    let x: T = std::default::Default::default();
                    entity_mut.insert(
                       x
                    );
                    comp = world.get_mut::<T>(entity);
                }
            }

            if let Some(mut comp) = comp {
                f(comp.as_mut());
                //comp.FixedWidth = fixed_width;
            }
        });
        self
    }

    fn h_small_spacing(&mut self) -> &mut Self {
        self.upsert(move |comp: &mut HList| {
            comp.spacing = SMALL_SPACE;
        })
    }

    fn h_spacing(&mut self, spacing: f32) -> &mut Self {
        self.upsert(move |comp: &mut HList| {
            comp.spacing = spacing;
        })
    }

    fn v_small_spacing(&mut self) -> &mut Self {
        self.upsert(move |comp: &mut VList| {
            comp.spacing = SMALL_SPACE;
        })
    }

    fn v_spacing(&mut self, spacing: f32) -> &mut Self {
        self.upsert(move |comp: &mut VList| {
            comp.spacing = spacing;
        })
    }


    fn stretch(&mut self) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.stretch();
        })
    }

    fn rounded(&mut self) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.BorderRadius = Vec4::splat(SMALL_SPACE);
        })
    }

    fn pill(&mut self) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.BorderRadius = Vec4::splat(SMALL_SPACE*100.0);
        })
    }

    fn on_show(&mut self, on_click: CommandFunc) -> &mut Self {
        self.insert(
            OnShow {
                func: Some(on_click),
                was_visible: false
            }
        )
    }

    fn shadow(&mut self) -> &mut Self {
        self.upsert(move |comp: &mut Shadow| {
        })
    }

    fn fixed_width(&mut self, fixed_width: f32) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.fixed_width = fixed_width;
        })
    }

    fn overflow(&mut self, is_overflow: bool) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.IsOverflow = is_overflow;
        })
    }

    fn fixed_size(&mut self, size: f32) -> &mut Self {
        self.fixed_height(size).fixed_width(size)
    }

    fn fixed_height(&mut self, fixed_height: f32) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.fixed_height = fixed_height;
        })
    }

    fn expand(&mut self) -> &mut Self {
        self.expand_height().expand_width()
    }

    fn expand_width(&mut self) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.ExpandWidth = true;
        })
    }

    fn ignore_layout(&mut self) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.ignore_layout = true;
        })
    }

    fn use_blur(&mut self) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.use_blur = true;
        })
    }

    fn z_index(&mut self, z_index: u32) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.z_index = Some(z_index);
        })
    }

    fn modify<F>(&mut self, func: F) -> &mut Self where F: FnOnce(&mut Self) -> &mut Self + Send + 'static {
        func(self)
    } 

    fn expand_height(&mut self) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.ExpandHeight = true;
        })
    }    

    fn is_visible(&mut self, is_visible: bool) -> &mut Self {
        self.upsert(move |comp: &mut Control| {
            comp.is_visible = is_visible;
        })
    }    

    fn align_text(&mut self, alignment: Anchor) -> &mut Self {
        self.upsert(move |comp: &mut BLabel| {
            comp.alignment = alignment;
        })
    }    

    fn search(&mut self) -> &mut Self {
        self.insert((
            Control {
                ExpandWidth: true,
                Padding: Vec4::splat(SMALL_SPACE),
                BorderRadius: Vec4::splat(10.0),
                ..default()
            },
            Container { ..default() },
            HList {
                spacing: SMALL_SPACE,
                ..default()
            },
            Shadow {},
            BackgroundColor(Color::WHITE)
        ))
        .with_children(|parent| {
            //parent.spawn((Control { Width: 5, ..default() }));
            //Some(DarkImageButton(parent, if (is_minimize) {"assets/icons/Minimize.png".to_string() } else { "assets/icons/Tasks.png".to_string() }, Some(|| { write_event(TASKS, "".to_string()); })));
            //parent.spawn((Control { Width: 15, ..default() }));

            parent.child().dark_image_button("assets/icons/".to_string() + "Search.png", None);
            let entity = parent.child().insert((
                Control {
                    ExpandWidth: true,
                    ..default()
                },
                InputField {
                    placeholder: "Search".to_string(),
                    input_type: InputType::Default,
                    ..default()
                },
                SearchInput {
                    ..default()
                }
            )).id();
            parent.child().v_list().bind_list(entity, "results", CreateEntityFunc::new(
                |commands| {
                    let mut child = commands.child();
                    child.label("".to_string(), DEFAULT_FONT_SIZE, Color::BLACK, Anchor::MiddleLeft, true).bind::<String>();
                    let entity = child.id(); //.bind_property(entity, "").id()
                    child.bind_property(entity, "");
                    entity
                }
            ));
            //let entity = child.id();
            //child.bind_property_with_func(entity, "Text", SetPropertyFunc::new(move|commands, _entity, reflect| {
            //}));
        })
    }

    fn chat_typing(&mut self, is_self: bool) -> &mut Self {
        // Main input field
        self.insert((
            Control {
                ExpandWidth: true,
                Padding: Vec4::splat(HALF_SMALL_SPACE),
                ..default()
            },
            Container { ..default() },
            HList {
                spacing: 0.0,
                anchor: if is_self { Anchor::MiddleRight } else { Anchor::MiddleLeft },
                ..default()
            }
        )).with_children(|parent| {
            parent.spawn((
                    Control {
                        //UseBackground: true,
                        fixed_height: 45.0,
                        Padding: Vec4::splat(SMALL_SPACE),
                        BorderRadius: if is_self { Vec4::new(15.0, 15.0, 15.0, 5.0) } else { Vec4::new(5.0, 15.0, 15.0, 15.0) },
                        ..default()
                    },
                    ImageRect {
                        image: "assets/icons/Typing.gif".to_string(),
                        brightness: 0.2,
                        ..default()
                    },
                    //BackgroundColor(if (is_self) { Srgba::hex("1C70FB").unwrap() } else { Color::WHITE }),
                    Shadow {},
                ))
                .id();
        })
    }
    
    fn plus_button(&mut self) -> &mut Self {
        self.fixed_size(MEDIUM_LARGE).h_list().small_padding().background_color(*GREEN).shadow().pill().with_children(|parent| {
            parent.child().image("assets/icons/Plus.png".to_string()).expand();
        })
    }

    fn medium_plus_button(&mut self) -> &mut Self {
        self.fixed_height(MEDIUM).fixed_width(MEDIUM).h_list().with_children(|parent| {
            parent.child().pill().fixed_height(MEDIUM-SMALL_SPACE).fixed_width(MEDIUM-SMALL_SPACE).insert((
                Shadow {},
                BackgroundColor(*GREEN)
            ))
            .with_children(|parent| {
                parent.spawn((
                    Control {
                        fixed_width: 17.0,
                        fixed_height: 17.0,
                        ..default()
                    },
                    ImageRect {
                        image: "assets/icons/Plus.png".to_string(),
                        ..default()
                    },
                ));
            });
        })
    }
    
    fn dark_image_button(
        &mut self,
        image: String,
        on_click: Option<CommandFunc>,
    ) -> &mut Self {
        let _image = image;
        self.insert((
            Control {
                BorderRadius: Vec4::splat((SMALL as f32) / 2.0),
                fixed_width: SMALL,
                fixed_height: SMALL,
                ..default()
            },
            BButton {
                on_click,
                ..default()
            },
        ))
        .with_children(|parent| {
            parent.spawn((
                Control {
                    fixed_width: SMALL,
                    fixed_height: SMALL,
                    ..default()
                },
                ImageRect {
                    image: _image,
                    brightness: 0.2,
                    ..default()
                },
            ));
        }).scale_on_hover()
    }

    fn medium_dark_image_button(
        &mut self,
        image: String
    ) -> &mut Self {
        self.insert((
            Control {
                BorderRadius: Vec4::splat((SMALL as f32) / 2.0),
                fixed_width: MEDIUM,
                fixed_height: MEDIUM,
                ..default()
            },
            Button {
                ..default()
            },
            ImageRect {
                image: image,
                brightness: 0.2,
                ..default()
            },
        )).padding(Vec4::splat(HALF_SMALL_SPACE*1.5)).scale_on_hover()
    }

    fn image_button(
        &mut self,
        image: String
    ) -> &mut Self {
        self.insert((
            Control {
                BorderRadius: Vec4::splat((SMALL as f32) / 2.0),
                fixed_width: SMALL,
                fixed_height: SMALL,
                ..default()
            },
            Button {
                ..default()
            },
            ImageRect {
                image: image,
                brightness: 1.0,
                ..default()
            },
            Shadow{}
        )).scale_on_hover()
    }
    
    fn slider(
        &mut self,
        percent: f64
    ) -> &mut Self {
        let mut fill_entity: Option<Entity> = None;
        self.insert((
            Control { ExpandWidth: true, fixed_height: SMALL_SPACE, BorderRadius: Vec4::splat(SMALL_SPACE/2.0), Padding: Vec4::new(0.0, 0.0, 0.0, 0.0), ..default() },
            Container { ..default() },
            HList { spacing: 0.0, anchor: Anchor::MiddleLeft, ..default() },
            BackgroundColor(Srgba::hex("b1acff").unwrap().into())
        )).with_children(|parent| {
            fill_entity = Some(parent.spawn((
                Control { fixed_width: 270.0*0.5, ExpandHeight: true, BorderRadius: Vec4::splat(SMALL_SPACE/2.0), ..default() },
                BackgroundColor(Srgba::hex("625AFAFF").unwrap().into()),
                Shadow {}
            )).id());
        }).insert(
            Slider { fill_entity: fill_entity, percent: 0.0 }
        )
    }

    fn stylized_title(&mut self, text: String) -> &mut Self {
        self.insert((
            Control {
                Padding: Vec4::new(0.0, 0.0, 0.0, 0.0),
                ExpandWidth: true,
                ..default()
            },
            BLabel {
                alignment: Anchor::MiddleCenter,
                text,
                //IsSingleLine: true,
                is_italic: true,
                font: "Mogra".to_string(),
                font_size: 25.0,
                color: Color::BLACK,
                ..default()
            },
        )).fixed_height(25.0)
        /*
        self.entity_commands.insert((
            Control {
                ExpandWidth: true,
                Padding: Vec4::splat(5.0),
                ..default()
            },
            Container {},
            HList {  Spacing: SMALL_SPACE, Anchor: Anchor::MiddleCenter, ..default() }
        ))
        .with_children(|parent| {

        });
         */
        //self
    }

    fn text_button(
        &mut self,
        label: String,
        color: Color
    ) -> &mut Self {
        self.insert((
            Control {
                BorderRadius: Vec4::splat(10.0),
                ..default()
            },
            Button {
                ..default()
            },
            Container {},
            VList { ..default() },
            Shadow {},
            BackgroundColor(color),
        ))
        .with_children(|parent| {
            parent.spawn((
                Control {
                    Padding: Vec4::splat(15.0),
                    ExpandWidth: true,
                    ..default()
                },
                BLabel {
                    alignment: Anchor::MiddleCenter,
                    text: label.to_string(),
                    is_single_line: true,
                    color: if color == Color::WHITE {
                        Color::BLACK
                    } else {
                        Color::WHITE
                    },
                    ..default()
                },
            ));
        }).scale_on_hover()
    }

    // TODO: Add code challenge
    // See for improving security: https://advancedweb.hu/how-to-secure-the-cognito-login-flow-with-a-state-nonce-and-pkce/
    // Official documentation: https://docs.aws.amazon.com/cognito/latest/developerguide/authorization-endpoint.html
    #[cfg(all(target_arch = "wasm32"))]
    fn google_button(&mut self) -> &mut Self {
        let origin = get_page_origin().unwrap().replace("http://127.0.0.1", "http://localhost");
        self.link_image_button( 
            "Sign in with Google".to_string(), 
            "assets/icons/Google.webp".to_string(), 
            Color::WHITE, 
            format!("https://oauth.empathic.social/oauth2/authorize?identity_provider=Google&redirect_uri={origin}/login&response_type=CODE&client_id=5jjqc5ebkpavqdsiq5lh18uh6q")
        ).scale_on_hover()
    }

    #[cfg(all(not(target_arch = "wasm32")))]
    fn google_button(&mut self) -> &mut Self {
        self
    }

    fn image_text_button(&mut self,
        image: String,
        label: String,
        color: Color,
        font_size: f32
    ) -> &mut Self {
        self.insert((
            Control {
                Padding: Vec4::splat(15.0),
                BorderRadius: Vec4::splat(10.0),
                //ExpandWidth: f,
                ..default()
            },
            Button { ..default() },
            Container {},
            HList {
                spacing: SMALL_SPACE,
                ..default()
            },
            Shadow {},
            BackgroundColor(color),
        ))
        .with_children(|parent| {
            parent
                .spawn((
                    Control {
                        fixed_width: SMALL,
                        fixed_height: SMALL,
                        ..default()
                    },
                    ImageRect {
                        image,
                        brightness: get_secondary_brightness(color),
                        ..default()
                    },
                ))
                .id();
            parent.spawn((
                Control {
                    //ExpandWidth: true,
                    ..default()
                },
                BLabel {
                    alignment: Anchor::MiddleCenter,
                    text: label.to_string(),
                    font_size,
                    is_single_line: true,
                    color: get_secondary_color(color),
                    ..default()
                },
            ));
        }).scale_on_hover()
    }

    #[cfg(all(target_arch = "wasm32"))]
    fn link_image_button(&mut self, label: String, image: String, color: Color, url: String) -> &mut Self {
    
        let url = url.clone();
        
        self.insert((
                Control {
                    BorderRadius: Vec4::splat(10.0),
                    ExpandWidth: true,
                    Padding: Vec4::splat(15.0),
                    ..default()
                },
                BButton {
                    on_click: Some(CommandFunc::new(move |_commands: &mut Commands| {
                        let url = url.clone();
                        spawn(async move {
                            go_to_url(url);
                        });
                    })),
                    ..default()
                },
                Container {},
                HList {  spacing: SMALL_SPACE, anchor: Anchor::MiddleCenter, ..default() },
                Shadow {},
                BackgroundColor(color),
            )).with_children(|parent| {
                parent.spawn((
                    Control {
                        fixed_width: 20.0,
                        fixed_height: 20.0,
                        ..default()
                    },
                    ImageRect {
                        image: image,
                        brightness: 1.0,
                        ..default()
                    },
                ));
                parent.spawn((
                    Control {
                        //ExpandWidth: true,
                        ..default()
                    },
                    BLabel {
                        alignment: Anchor::MiddleCenter,
                        text: label.to_string(),
                        //IsSingleLine: true,
                        color: get_secondary_color(color),
                        ..default()
                    },
                ));
            })
    }
    
    /* 
    // Method to add a custom step
    fn add_custom_step<F>(mut self, step: F) -> Self 
    where
        F: Fn(&mut EntityCommands) + 'a,
    {
        //self.custom_steps.push(Box::new(step));
        self
    }
    */
    // Method to build the entity and apply custom steps
    fn id(&mut self) -> Entity {
        //let mut entity_commands = self.parent.spawn((
            // ... initialize components
        //));
        
        // Apply custom steps
        //for step in self.custom_steps {
        //    step(&mut entity_commands);
        //}
        
        self.get_commands().id()
    }
}

impl<'a> BaseBuilder<'a> for EntityBuilder<'a> {
}
