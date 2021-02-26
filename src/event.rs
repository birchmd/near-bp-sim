use crate::id::Id;
use crate::role::Role;

pub trait EventConsumer {
    fn push(&mut self, event: Event);
}

#[derive(Debug)]
pub struct Event {
    pub time: usize,
    pub info: Info,
}

#[derive(Debug)]
pub enum Info {
    // New participant introduced. Their initial role is always `None`.
    ParticipantCreated {
        participant_id: Id,
        num_tokens: f64,
    },
    StakeChange {
        participant_id: Id,
        change_amount: f64,
    },
    RoleChange {
        participant_id: Id,
        new_role: Option<Role>,
    },
    // Two participants pool their tokens together.
    ParticipantsMerged {
        participant_ids: (Id, Id),
        new_participant_id: Id,
    },
    // One participant splits their tokens evenly between
    // two new participants.
    ParticipantSplit {
        participant_id: Id,
        new_participant_ids: (Id, Id),
    },
    // Participants with less than or equal to 0 tokens are
    // removed. This is occurrence is recorded by this event.
    ParticipantBankrupt {
        participant_id: Id,
    },
}

#[derive(Default)]
pub struct EventAccumulator {
    pub events: Vec<Event>,
}

impl EventConsumer for EventAccumulator {
    fn push(&mut self, event: Event) {
        self.events.push(event);
    }
}

pub struct EventBlackHole;

impl EventConsumer for EventBlackHole {
    fn push(&mut self, _event: Event) {}
}
