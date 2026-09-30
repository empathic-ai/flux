use bevy::prelude::{App, Events, Update};
use flux::prelude::*;
use futures_util::Stream;
use std::{
    pin::Pin,
    sync::{Arc, atomic::{AtomicUsize, Ordering}},
    task::{Context, Poll, Wake, Waker},
};

fn event(sender: Id) -> NetworkEvent {
    NetworkEvent::new(sender, TrackRecordEvent { entity_id: Id::new() })
}

struct CountWake(AtomicUsize);

impl Wake for CountWake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn late_receiver_gets_first_live_event_after_old_events_are_consumed() {
    let multiplexer = Multiplexer::new();
    let recipient = Id::new();
    let sender = Id::new();
    let mut first = multiplexer.get_channel(recipient);
    multiplexer.send(recipient, event(sender));
    assert!(first.try_recv().is_some());
    let mut late = multiplexer.get_channel(recipient);
    multiplexer.send(recipient, event(sender));
    assert!(late.try_recv().is_some());
    assert!(first.try_recv().is_some());
}

#[test]
fn sending_wakes_every_pending_receiver() {
    let multiplexer = Multiplexer::new();
    let recipient = Id::new();
    let mut first = multiplexer.get_channel(recipient);
    let mut second = multiplexer.get_channel(recipient);
    let first_wake = Arc::new(CountWake(AtomicUsize::new(0)));
    let second_wake = Arc::new(CountWake(AtomicUsize::new(0)));
    let first_waker = Waker::from(first_wake.clone());
    let second_waker = Waker::from(second_wake.clone());
    assert!(matches!(Pin::new(&mut first).poll_next(&mut Context::from_waker(&first_waker)), Poll::Pending));
    assert!(matches!(Pin::new(&mut second).poll_next(&mut Context::from_waker(&second_waker)), Poll::Pending));
    multiplexer.send(recipient, event(Id::new()));
    assert_eq!(first_wake.0.load(Ordering::Relaxed), 1);
    assert_eq!(second_wake.0.load(Ordering::Relaxed), 1);
    assert!(first.try_recv().is_some());
    assert!(second.try_recv().is_some());
}

#[test]
fn a_slow_receiver_skips_oldest_events_but_remains_live() {
    let multiplexer = Multiplexer::new();
    let recipient = Id::new();
    let sender = Id::new();
    let mut slow = multiplexer.get_channel(recipient);
    for _ in 0..1025 {
        multiplexer.send(recipient, event(sender));
    }
    assert_eq!((0..1025).filter(|_| slow.try_recv().is_some()).count(), 1024);
    multiplexer.send(recipient, event(sender));
    assert!(slow.try_recv().is_some());
}

#[test]
fn ecs_relay_drains_all_queued_events_in_one_update() {
    let recipient = Id::new();
    let sender = Id::new();
    let mut app = App::new();
    app.add_event::<PeerEvent>()
        .add_event::<NetworkEvent>()
        .insert_resource(Session::new(recipient))
        .add_systems(Update, relay_network_events);

    let multiplexer = app.world().resource::<Session>().get_multiplexer();
    for _ in 0..3 {
        multiplexer.send(recipient, event(sender));
    }
    app.update();

    let events = app.world().resource::<Events<NetworkEvent>>();
    assert_eq!(events.get_cursor().read(events).count(), 3);
}

#[test]
fn receivers_keep_sequence_after_one_receiver_drops() {
    let multiplexer = Multiplexer::new();
    let recipient = Id::new();
    let mut first = multiplexer.get_channel(recipient);
    let mut second = multiplexer.get_channel(recipient);
    let ids: Vec<_> = (0..3).map(|_| Id::new()).collect();
    for id in &ids {
        multiplexer.send(recipient, NetworkEvent::new(Id::new(), TrackRecordEvent { entity_id: *id }));
    }
    assert_eq!(first.try_recv().unwrap().get_ev::<TrackRecordEvent>().unwrap().entity_id, ids[0]);
    assert_eq!(second.try_recv().unwrap().get_ev::<TrackRecordEvent>().unwrap().entity_id, ids[0]);
    assert_eq!(first.try_recv().unwrap().get_ev::<TrackRecordEvent>().unwrap().entity_id, ids[1]);
    drop(first);
    assert_eq!(second.try_recv().unwrap().get_ev::<TrackRecordEvent>().unwrap().entity_id, ids[1]);
    assert_eq!(second.try_recv().unwrap().get_ev::<TrackRecordEvent>().unwrap().entity_id, ids[2]);
    assert!(second.try_recv().is_none());
}

#[test]
fn late_receiver_does_not_erase_new_event_for_earlier_receiver() {
    let multiplexer = Multiplexer::new();
    let recipient = Id::new();
    let first_id = Id::new();
    let second_id = Id::new();
    let mut earlier = multiplexer.get_channel(recipient);
    multiplexer.send(recipient, NetworkEvent::new(Id::new(), TrackRecordEvent { entity_id: first_id }));
    let mut late = multiplexer.get_channel(recipient);
    multiplexer.send(recipient, NetworkEvent::new(Id::new(), TrackRecordEvent { entity_id: second_id }));

    assert_eq!(late.try_recv().unwrap().get_ev::<TrackRecordEvent>().unwrap().entity_id, second_id);
    assert!(late.try_recv().is_none());
    assert_eq!(earlier.try_recv().unwrap().get_ev::<TrackRecordEvent>().unwrap().entity_id, first_id);
    assert_eq!(earlier.try_recv().unwrap().get_ev::<TrackRecordEvent>().unwrap().entity_id, second_id);
    assert!(earlier.try_recv().is_none());
}