use crate::id::Id;
use crate::role::Role;

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::Path;

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

#[derive(Default)]
pub struct StatsAccumulator {
    history: Vec<Stats>,
    current: Stats,
    stakes: HashMap<Id, f64>,
    roles: HashMap<Id, Role>,
}

#[derive(Debug, Default, Clone)]
pub struct Stats {
    time: usize,
    total_bp_stake: f64,
    total_cop_stake: f64,
    total_delegated_bp_stake: f64,
    total_delegated_cop_stake: f64,
}

impl StatsAccumulator {
    pub fn write_stats<P: AsRef<Path>>(&mut self, file_name: P) -> std::io::Result<()> {
        let mut file = File::create(file_name)?;
        file.write(b"time,total_bp_stake,total_cop_stake,total_delegated_bp_stake,total_delegated_cop_stake\n")?;
        for s in self.history.iter() {
            let line = format!(
                "{},{},{},{},{}\n",
                s.time,
                s.total_bp_stake,
                s.total_cop_stake,
                s.total_delegated_bp_stake,
                s.total_delegated_cop_stake
            );
            file.write(&line.as_bytes())?;
        }
        self.compute_totals();
        let line = format!(
            "{},{},{},{},{}\n",
            self.current.time,
            self.current.total_bp_stake,
            self.current.total_cop_stake,
            self.current.total_delegated_bp_stake,
            self.current.total_delegated_cop_stake
        );
        file.write(&line.as_bytes())?;
        Ok(())
    }

    fn compute_totals(&mut self) {
        self.current.total_bp_stake = 0.0;
        self.current.total_cop_stake = 0.0;
        self.current.total_delegated_bp_stake = 0.0;
        self.current.total_delegated_cop_stake = 0.0;

        for (id, stake) in self.stakes.iter() {
            if let Some(role) = self.roles.get(id) {
                match role {
                    Role::BlockProducer => self.current.total_bp_stake += stake,
                    Role::ChunkOnlyProducer => self.current.total_cop_stake += stake,
                    Role::Delegator(delegatee_id) => match self.roles.get(delegatee_id) {
                        Some(Role::BlockProducer) => {
                            self.current.total_bp_stake += stake;
                            self.current.total_delegated_bp_stake += stake;
                        }
                        Some(Role::ChunkOnlyProducer) => {
                            self.current.total_cop_stake += stake;
                            self.current.total_delegated_cop_stake += stake;
                        }
                        None | Some(Role::Delegator(_)) => (),
                    },
                }
            }
        }
    }

    fn remove_stake_or_default(&mut self, participant_id: &Id) -> f64 {
        self.stakes.remove(&participant_id).unwrap_or(0.0)
    }
}

impl EventConsumer for StatsAccumulator {
    fn push(&mut self, e: Event) {
        if e.time != self.current.time {
            self.compute_totals();
            self.history.push(self.current.clone());
            self.current.time = e.time;
        }

        match e.info {
            Info::ParticipantCreated {
                participant_id,
                num_tokens,
            } => {
                self.stakes.insert(participant_id, num_tokens);
            }
            Info::StakeChange {
                participant_id,
                change_amount,
            } => {
                *self
                    .stakes
                    .get_mut(&participant_id)
                    .expect("Participant must be created before stake is changed") += change_amount;
            }
            Info::RoleChange {
                participant_id,
                new_role,
            } => match new_role {
                None => {
                    self.roles.remove(&participant_id);
                }
                Some(role) => {
                    self.roles.insert(participant_id, role);
                }
            },
            Info::ParticipantsMerged {
                new_participant_id,
                participant_ids,
            } => {
                let new_stake = self.remove_stake_or_default(&participant_ids.0)
                    + self.remove_stake_or_default(&participant_ids.1);
                self.stakes.insert(new_participant_id, new_stake);
                let role0 = self.roles.remove(&participant_ids.0);
                let role1 = self.roles.remove(&participant_ids.1);
                debug_assert!(role0 == role1);
                if let Some(role) = role0 {
                    self.roles.insert(new_participant_id, role);
                }
            }
            Info::ParticipantSplit {
                participant_id,
                new_participant_ids,
            } => {
                if let Some(role) = self.roles.remove(&participant_id) {
                    self.roles.insert(new_participant_ids.0, role);
                    self.roles.insert(new_participant_ids.1, role);
                }
                if let Some(stake) = self.stakes.remove(&participant_id) {
                    self.stakes.insert(new_participant_ids.0, stake / 2.0);
                    self.stakes.insert(new_participant_ids.1, stake / 2.0);
                }
            }
            Info::ParticipantBankrupt { participant_id } => {
                self.roles.remove(&participant_id);
            }
        }
    }
}
