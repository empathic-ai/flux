use ::serde::{Deserialize, Deserializer, Serialize, Serializer};
use bevy_reflect::*;

/// An owned reflected value. Bindings unwrap this slot so paths continue
/// directly through `ReactiveView.value`.
#[derive(Debug, Reflect)]
#[reflect(opaque)]
#[reflect(Clone, Serialize, Deserialize)]
pub struct Dynamic(pub Box<dyn PartialReflect>);

impl Dynamic {
    pub fn new<T: PartialReflect + ?Sized>(value: &T) -> Self {
        Self(Self::unwrap(value.clone_value()))
    }

    /// Extract the contents when reading a binding, including nested wrappers.
    pub(crate) fn unwrap(mut value: Box<dyn PartialReflect>) -> Box<dyn PartialReflect> {
        while let Some(dynamic) = value.try_downcast_ref::<Self>() {
            value = dynamic.0.clone_value();
        }
        value
    }
}

impl AsRef<dyn PartialReflect> for Dynamic {
    fn as_ref(&self) -> &(dyn PartialReflect + 'static) {
        self.0.as_ref()
    }
}

impl AsMut<dyn PartialReflect> for Dynamic {
    fn as_mut(&mut self) -> &mut (dyn PartialReflect + 'static) {
        self.0.as_mut()
    }
}

impl Clone for Dynamic {
    fn clone(&self) -> Self {
        Self::new(self.0.as_ref())
    }
}

impl Serialize for Dynamic {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let registry = value_registry();
        serde::ReflectSerializer::new(self.0.as_ref(), &registry).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Dynamic {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use ::serde::de::DeserializeSeed;
        let registry = value_registry();
        Ok(Self(Self::unwrap(
            serde::ReflectDeserializer::new(&registry).deserialize(deserializer)?,
        )))
    }
}

fn value_registry() -> TypeRegistry {
    let mut registry = TypeRegistry::new();
    crate::types::dynamic_struct_serde::register_common_types(&mut registry);
    // Support primitive rows without requiring application-side registration.
    registry.register::<()>();
    registry.register::<bool>();
    registry.register::<char>();
    registry.register::<i8>();
    registry.register::<i16>();
    registry.register::<i32>();
    registry.register::<i64>();
    registry.register::<i128>();
    registry.register::<isize>();
    registry.register::<u8>();
    registry.register::<u16>();
    registry.register::<u32>();
    registry.register::<u64>();
    registry.register::<u128>();
    registry.register::<usize>();
    registry.register::<f32>();
    registry.register::<f64>();
    registry
}

/*
#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::{Id, ReactiveView, apply_value};

    // As with DynamicStruct, non-scalar payload types must be registered on
    // the receiving side. Reactive derives do this for application records.
    reflect_steroids::enable_global_type_registration!(Vec<i32>);

    fn round_trip<T: Reflect + FromReflect + PartialEq + std::fmt::Debug>(value: T) {
        let view = ReactiveView {
            value: Dynamic::new(&value),
        };
        let cloned = ReactiveView::from_reflect(view.clone_value().as_ref()).unwrap();
        assert_eq!(T::from_reflect(cloned.value.as_ref()), Some(value));
        let bytes = postcard::to_allocvec(&view).unwrap();
        let decoded: ReactiveView = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(
            view.value
                .as_ref()
                .reflect_partial_eq(decoded.value.as_ref()),
            Some(true)
        );
        let json = serde_json::to_string(&view).unwrap();
        let decoded: ReactiveView = serde_json::from_str(&json).unwrap();
        assert_eq!(
            view.value
                .as_ref()
                .reflect_partial_eq(decoded.value.as_ref()),
            Some(true)
        );
    }

    #[test]
    fn view_values_round_trip_structs_and_scalars() {
        round_trip(Id::new());
        round_trip(uuid::Uuid::from_u128(42));
        round_trip("network name".to_string());
        round_trip(42_i32);
        round_trip(true);
        round_trip(1.5_f64);
        round_trip(vec![1_i32, 2, 3]);
        round_trip(Some("network".to_string()));
    }

    #[test]
    fn struct_view_serialization_preserves_existing_format() {
        #[derive(Serialize, Deserialize)]
        struct OldView {
            #[serde(with = "crate::types::dynamic_struct_serde")]
            value: DynamicStruct,
        }
        let id = Id::new();
        let old = OldView {
            value: id.to_dynamic_struct(),
        };
        let new = ReactiveView {
            value: Dynamic::new(&id),
        };
        let bytes = postcard::to_allocvec(&old).unwrap();
        assert_eq!(bytes, postcard::to_allocvec(&new).unwrap());
        let decoded: ReactiveView = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(Id::from_reflect(decoded.value.as_ref()), Some(id));
        let old: OldView = postcard::from_bytes(&postcard::to_allocvec(&new).unwrap()).unwrap();
        assert_eq!(Id::from_reflect(&old.value), Some(id));
    }

    #[test]
    fn view_values_round_trip_through_reflection() {
        use ::serde::de::DeserializeSeed;
        let view = ReactiveView {
            value: Dynamic::new(&uuid::Uuid::from_u128(42)),
        };
        let mut registry = value_registry();
        registry.register::<ReactiveView>();
        for value in [&view as &dyn PartialReflect, view.clone_value().as_ref()] {
            let json =
                serde_json::to_string(&serde::ReflectSerializer::new(value, &registry)).unwrap();
            let mut deserializer = serde_json::Deserializer::from_str(&json);
            let decoded = serde::ReflectDeserializer::new(&registry)
                .deserialize(&mut deserializer)
                .unwrap();
            let decoded = ReactiveView::from_reflect(decoded.as_ref()).unwrap();
            assert_eq!(
                uuid::Uuid::from_reflect(decoded.value.as_ref()),
                Some(uuid::Uuid::from_u128(42))
            );
            let bytes =
                postcard::to_allocvec(&serde::ReflectSerializer::new(value, &registry)).unwrap();
            let mut deserializer = postcard::Deserializer::from_bytes(&bytes);
            let decoded = serde::ReflectDeserializer::new(&registry)
                .deserialize(&mut deserializer)
                .unwrap();
            let decoded = ReactiveView::from_reflect(decoded.as_ref()).unwrap();
            assert_eq!(
                uuid::Uuid::from_reflect(decoded.value.as_ref()),
                Some(uuid::Uuid::from_u128(42))
            );
        }
    }

    #[test]
    fn dynamic_assignment_replaces_types_options_and_collections() {
        let mut value = Dynamic::new(&Id::nil());
        apply_value(&mut value, 7_i32).unwrap();
        assert_eq!(i32::from_reflect(value.as_ref()), Some(7));
        apply_value(&mut value, Some("network".to_string())).unwrap();
        apply_value(&mut value, None::<String>).unwrap();
        assert_eq!(Option::<String>::from_reflect(value.as_ref()), Some(None));
        apply_value(&mut value, vec![1, 2, 3]).unwrap();
        apply_value(&mut value, vec![9]).unwrap();
        assert_eq!(Vec::<i32>::from_reflect(value.as_ref()), Some(vec![9]));
        apply_value(&mut value, Vec::<i32>::new()).unwrap();
        assert_eq!(Vec::<i32>::from_reflect(value.as_ref()), Some(vec![]));
    }
}
*/