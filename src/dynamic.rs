use bevy::reflect::*;
use bevy::prelude::*;
use prost::Message;
use prost::encoding::WireType;
use prost::encoding::DecodeContext;
use prost::Name;
use prost_types::Any;
use prost::encoding::skip_field;

pub trait ReflectMessage: Reflect + Message {
    fn encode_boxed(&self) -> Vec<u8>;

    fn full_name(&self) -> String;

    fn type_url(&self) -> String;
}

#[derive(Reflect, Debug)]
pub struct Empty {

}

impl ReflectMessage for Empty {
    fn full_name(&self) -> String {
        todo!()
    }

    fn type_url(&self) -> String {
        todo!()
    }
    
    fn encode_boxed(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.encode(&mut buf).unwrap();
        buf
    }
}

impl Message for Empty {
    fn encoded_len(&self) -> usize {
        0
    }

    fn clear(&mut self) {
    }
    
    fn encode_raw(&self, buf: &mut impl prost::bytes::BufMut)
    where
        Self: Sized {
    }
    
    fn merge_field(
        &mut self,
        tag: u32,
        wire_type: WireType,
        buf: &mut impl prost::bytes::Buf,
        ctx: DecodeContext,
    ) -> Result<(), prost::DecodeError>
    where
        Self: Sized {
            skip_field(wire_type, tag, buf, ctx)
    }
}

#[derive(Debug, TypePath)]
pub struct Dynamic {
    pub value: Box<dyn ReflectMessage>,
    //pub cloned_func: Arc<dyn CloneBoxedCloneFunc>
}

impl Default for Dynamic {
    fn default() -> Self {
        Self { value: Box::new(Empty {}) }
    }
}

impl Message for Dynamic {
    fn encode_raw(&self, buf: &mut impl prost::bytes::BufMut)
    where
        Self: Sized {
        
        let o = self.value.as_ref();

        let type_url = o.type_url();

        let value = o.encode_boxed();

        let any = Any { type_url, value };
        
        any.encode(buf).unwrap();
    }

    fn merge_field(
        &mut self,
        tag: u32,
        wire_type: WireType,
        buf: &mut impl prost::bytes::Buf,
        ctx: DecodeContext,
    ) -> Result<(), prost::DecodeError>
    where
        Self: Sized {

        //self.value.as_ref().merge(buf)
        todo!()
    }

    fn encoded_len(&self) -> usize {
        todo!()
    }

    fn clear(&mut self) {
        todo!()
    }
}

impl PartialReflect for Dynamic {
    fn get_represented_type_info(&self) -> Option<&'static TypeInfo> {
        self.value.get_represented_type_info()
    }
    
    fn into_partial_reflect(self: Box<Self>) -> Box<dyn PartialReflect> {
        self.value.into_partial_reflect()
    }
    
    fn as_partial_reflect(&self) -> &dyn PartialReflect {
        self.value.as_partial_reflect()
    }
    
    fn as_partial_reflect_mut(&mut self) -> &mut dyn PartialReflect {
        self.value.as_partial_reflect_mut()
    }
    
    fn try_into_reflect(self: Box<Self>) -> Result<Box<dyn Reflect>, Box<dyn PartialReflect>> {
        todo!()
    }
    
    fn try_as_reflect(&self) -> Option<&dyn Reflect> {
        todo!()
    }
    
    fn try_as_reflect_mut(&mut self) -> Option<&mut dyn Reflect> {
        todo!()
    }
    
    fn try_apply(&mut self, value: &dyn PartialReflect) -> Result<(), ApplyError> {
        todo!()
    }
    
    fn reflect_ref(&self) -> ReflectRef {
        todo!()
    }
    
    fn reflect_mut(&mut self) -> ReflectMut {
        todo!()
    }
    
    fn reflect_owned(self: Box<Self>) -> ReflectOwned {
        todo!()
    }
    
    fn clone_value(&self) -> Box<dyn PartialReflect> {
        todo!()
    }
    /*
    fn get_represented_type_info(&self) -> Option<&'static TypeInfo> {
        self.value.get_represented_type_info()
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self.value.into_any()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self.value.as_any()
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self.value.as_any_mut()
    }

    fn into_reflect(self: Box<Self>) -> Box<dyn Reflect> {
        self.value.into_reflect()
    }

    fn as_reflect(&self) -> &dyn Reflect {
        self.value.as_reflect()
    }

    fn as_reflect_mut(&mut self) -> &mut dyn Reflect {
        self.value.as_reflect_mut()
    }

    fn apply(&mut self, value: &dyn Reflect) {
        self.value.apply(value)
    }

    fn set(&mut self, value: Box<dyn Reflect>) -> std::prelude::v1::Result<(), Box<dyn Reflect>> {
        self.value.set(value)
    }

    fn reflect_ref(&self) -> ReflectRef {
        self.value.reflect_ref()
    }

    fn reflect_mut(&mut self) -> ReflectMut {
        self.value.reflect_mut()
    }

    fn reflect_owned(self: Box<Self>) -> bevy::reflect::ReflectOwned {
        self.value.reflect_owned()
    }

    fn clone_value(&self) -> Box<dyn Reflect> {
        self.value.clone_value()
    }
    
    fn try_apply(&mut self, value: &dyn Reflect) -> std::result::Result<(), bevy::reflect::ApplyError> {
        self.value.try_apply(value)
    } */
}

/*
impl FromReflect for Dynamic {
    fn from_reflect(val: &(dyn bevy::prelude::Reflect + 'static)) -> std::option::Option<Self> {
        Some(Dynamic::from_reflect(val.clone_value()))
    }
} */

impl Dynamic {
    pub fn new<T>(value: T) -> Dynamic where T: ReflectMessage {
        Dynamic {
            value: Box::new(value)
            /* ,
            cloned_func: Arc::new(|value| {
                if let Some(val) = value.downcast_ref::<T>() {
                    return Box::new(val.clone());
                } else {
                    panic!("Wrong type!")
                }
            })
            */
        }
    }
    pub fn from_reflect(value: Box<dyn ReflectMessage>) -> Dynamic {
        Dynamic {
            value: value
            /*
            cloned_func: Arc::new(|value: &Box<dyn Reflect>| {
                let clone = value as &Clone;
                clone.clone()
                //value.clone_into(target)
            })
            */
        }
    }
    pub fn cast<T>(self) -> Option<T> where T: ReflectMessage + FromReflect + Typed {
        dbg!(self.value.reflect_type_path());
        dbg!(T::type_info().type_path());

        if self.value.reflect_type_path() == T::type_info().type_path() {
            T::from_reflect(self.value.as_reflect())
        } else {
            None
        }
    }
}
/*
impl Clone for Dynamic {
    fn clone(&self) -> Self {
        let value = self.value.clone();
        Self {
            value: value
            //cloned_func: self.cloned_func.clone()
        }
    }
} */