use bevy::reflect::DynamicStruct;
use bevy::reflect::ReflectRef;
use bevy::reflect::TypeInfo;
use bevy::reflect::Typed;
//use tokio::sync::broadcast;
//use tokio::sync::broadcast::Sender;
//use tokio::sync::broadcast::Receiver;
#[cfg(feature = "futures")]
use futures_util::Stream;
//use futures::TryStreamExt;
//use futures_util::stream::BoxStream;
//use futures_util::sink::Drain;

use uuid::Uuid;
use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Poll, Context, Waker};
use anyhow::Result;
use crate::prelude::*;
use bevy::prelude::*;

#[derive(Clone)]
pub struct Multiplexer {
    channels: Arc<RwLock<HashMap<Id, MultiplexerChannel>>>
}

#[derive(Default)]
pub struct MultiplexerChannel {
    peer_id: Id,
    // The last event index for this multiplexer channel
    last_ev: usize,
    num_receivers: usize,
    buffer: Vec<(usize, NetworkEvent)>,
    /// Registered waker that will be called when a new event is sent.
    waker: Option<Waker>,
}

impl MultiplexerChannel {
    pub fn new(peer_id: Id) -> Self {
        Self {
            peer_id,
            ..default()
        }
    }

    pub fn send_ev(&mut self, ev: NetworkEvent) {
        if self.num_receivers > 0 {
            self.last_ev += 1;
            self.buffer.push((self.num_receivers, ev));

            if let Some(waker) = self.waker.take() {
                waker.wake();
            }
        } else {
            //info!("No receivers found! {:?}", ev);
        }
    }

    pub fn recv_ev(&mut self, mut last_ev: &mut usize) -> Option<NetworkEvent> {
        let ev_dif = self.last_ev - *last_ev;

        if ev_dif == 0 {
            return None;
        }

        //info!("Channel receiver count: {}", self.num_receivers.clone());
        //info!("Channel last ev: {}", self.last_ev.clone());
        //info!("Receiver last ev: {}", last_ev.clone());

        //info!("Channel ID: {}", self.peer_id);
        //info!("Channel buffer length: {}", self.buffer.len());
        //for (num_receivers, ev) in &self.buffer {
        //    info!("{}", ev.get_ev_name());
        //}

        let ev_index = self.buffer.len()-ev_dif;
        let (lock_count, ev) = &mut self.buffer[ev_index];

        let ev = ev.clone();

        *lock_count -= 1;

        if *lock_count == 0 {
            self.buffer.remove(ev_index);
        }
        
        *last_ev += 1;
        Some(ev)
    }
}

pub struct Channel {
    id: Id,
    last_ev: usize,
    multiplexer: Multiplexer
}

impl Clone for Channel {
    fn clone(&self) -> Self {
        self.multiplexer.get_channel(self.get_id())
    }
}

// when the streaming body is dropped, this runs
impl Drop for Channel {
    fn drop(&mut self) {
        let mut channels = self.multiplexer.channels.write().unwrap();
        if let Some(ch) = channels.get_mut(&self.id) {
            // decrement the number of active receivers
            if ch.num_receivers > 0 {
                ch.num_receivers -= 1;
            }
            // optional: if no receivers left and no buffered events, remove the entry
            if ch.num_receivers == 0 && ch.buffer.is_empty() {
                channels.remove(&self.id);
            }
        }
    }
}

impl Channel {

    pub fn get_id(&self) -> Id {
        self.id.clone()
    }

    pub fn send_ev<T>(&self, recipient_id: Id, ev: T) where T: Struct {
        self.multiplexer.send(recipient_id, NetworkEvent::new(self.id.clone(), ev));
    }

    pub fn try_recv(&mut self) -> Option<NetworkEvent> {
        let mut channels = self.multiplexer.channels.write().unwrap();

        let mut channel = channels.get_mut(&self.id.clone()).expect(&format!("Failed to get peer event buffer for ID {}", self.id.id));

        //dbg!(self.peer_id.clone());
        if let Some(ev) = channel.recv_ev(&mut self.last_ev) {
            return Some(ev);
        }

        None
        /*
        let sender = self.map.read().unwrap().get(&recv_id).unwrap().to_owned();
        
        let ev = sender.subscribe().try_recv();
        if let Ok(ev) = ev {
            return Some(ev);
        }
        None
        */
    }
}

#[cfg(feature = "futures")]
impl Stream for Channel {
    type Item = NetworkEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Acquire the lock on the underlying multiplexer channel.
        let multiplxer = self.multiplexer.clone();
        let mut channels = multiplxer.channels.write().unwrap();
        let ch = channels.get_mut(&self.id).expect(&format!("Failed to get peer event buffer for ID {}", self.id.id));
        
        // First check: try to receive an event.
        //info!("Poll receiving...");
        if let Some(ev) = ch.recv_ev(&mut self.last_ev) {
            return Poll::Ready(Some(ev));
        }
        
        // No event yet: register the waker.
        ch.waker = Some(cx.waker().clone());
        
        /*
        // Double-check: an event could have been sent right after the previous check.
        info!("Waker woke up, receiving...");
        if let Some(ev) = ch.recv_ev(self.last_ev) {
            self.last_ev += 1;
            // Clear the waker because we are about to return an event.
            ch.waker = None;
            return Poll::Ready(Some(ev));
        } */
        
        Poll::Pending
    }
}

impl Multiplexer {
    pub fn new() -> Self {
        Self {
            //map: Arc::new(RwLock::new(HashMap::new())),
            channels: Arc::new(RwLock::new(HashMap::new()))
        }
    }

    /*
    pub fn add(&mut self, peer_id: String) -> Result<()> {
        //let (tx, _) = broadcast::channel::<NetworkEvent>(10);
        //self.map.write().unwrap().insert(peer_id, tx);
        self.channels.write().unwrap().insert(peer_id, MultiplexerChannel::default());
        Ok(())
    }*/

    pub fn get_channel(&self, peer_id: Id) -> Channel {
        let mut channels = self.channels.write().unwrap();
        let mut channel = channels.entry(peer_id.clone()).or_insert_with(|| MultiplexerChannel::new(peer_id.clone()));
        channel.num_receivers += 1;
        //info!("Added receiver for {}.", peer_id);
        
        Channel {
            last_ev: channel.last_ev,
            id: peer_id,
            multiplexer: self.clone()
        }
    }

    /*
    // TODO: move into helper function elsewhere
    // Used for checking if type T is a certian variant
    pub fn try_recv<T>(&mut self, peer_id: Uuid) -> Option<NetworkEvent> where T: Typed {
        let mut buffer = self.buffer.write().unwrap();
        let mut buffer = buffer.get_mut(&peer_id).unwrap();
        if let Some(index) = buffer.iter().position(|x| {
            if let ReflectRef::Enum(enum_ref) = x.ev.as_reflect().reflect_ref() {
                
                let type_registry = TypeRegistry::default();
                if let TypeInfo::Enum(enum_info) = type_registry.get_type_info(enum_ref.type_id()).unwrap() {
                    enum_info.variant_at(index)
                }
                
                
                //if let VariantType::Tuple(struct_type) = enum_ref.variant()
                    
                //}
            }

            //DynamicEnum::
            //if let TypeInfo::Enum(enum_info) = type_registry.get_type_info(x.ev.type_id()) {
            //    enum_info.
            //}
           
            if x.ev.type_id() == T::type_info().type_id() {

            }
            true
        }) {
            vec.remove(index);
        }
        buffer.pop()
        
    }
    */

    //pub async fn recv(&mut self, recv_id: Uuid) -> Result<NetworkEvent> {
    //    let sender = self.map.read().unwrap().get(&recv_id).unwrap().to_owned();
    //    Ok(sender.subscribe().recv().await?)
    //}

    pub fn send(&self, recipient_id: Id, ev: NetworkEvent) {
        //dbg!(recv_id);
        //let sender = self.map.read().unwrap().get(&recv_id).unwrap()./to_owned();
        //sender.send(ev.clone()).unwrap();

        let mut channels = self.channels.write().unwrap();
        channels.entry(recipient_id.clone()).or_insert_with(move || MultiplexerChannel::new(recipient_id)).send_ev(ev);
    }

    pub fn send_ev<T>(&self, sender_id: Id, receiver_id: Id, ev: T) where T: Struct {
        self.send(receiver_id, NetworkEvent::new(sender_id, ev));
    }

    pub async fn recv_ev<T>(&self, receiver_id: Id, sender_id: Id) -> Result<T> where T: Reflect + FromReflect + Typed {
        let mut rx = self.get_channel(receiver_id);
        loop {
            if let Some(ev) = rx.try_recv() {
                if ev.peer_id == sender_id {
                    let event_type = ev.ev;

                    if let ReflectRef::Enum(enum_ref) = event_type.reflect_ref() {
                        let s = enum_ref.field_at(0).unwrap();

                        if let TypeInfo::Struct(_struct_info) = T::type_info()
                        {
                            if _struct_info.type_path() == s.reflect_type_path() {

                                let mut t = T::from_reflect(s).unwrap();
                   
                                //let mut t = T::default();
                                //t.apply(s);
                                
                                return Ok(t); 
                            }
                        }
                    }
                }
            }
        }
    }
}