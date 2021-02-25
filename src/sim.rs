use crate::event::{self, Event, EventConsumer};
use crate::id::{Id, IdGenerator};
use crate::role::Role;

use rand::Rng;

use std::collections::HashMap;

pub struct Params {
    pub num_block_producers: usize,
    pub num_chunk_only_producers: usize,
    pub chunk_only_producer_cost: f64,
    pub block_producer_cost_factor: f64,
    pub total_reward: f64,
    pub block_producer_reward_fraction: f64,
    pub block_producer_delegation_fee: f64,
    pub chunk_only_producer_delegation_fee: f64,
}

pub struct Simulation {
    participants: HashMap<Id, Participant>,
    params: Params,
    id_generator: IdGenerator,
}

impl Simulation {
    pub fn new(initial_stakes: &[f64], params: Params) -> Self {
        let mut id_generator = IdGenerator::default();
        let participants = initial_stakes
            .iter()
            .map(|stake| {
                let p = Participant::new(&mut id_generator, *stake);
                (p.id, p)
            })
            .collect();
        Self {
            participants,
            params,
            id_generator,
        }
    }

    pub fn run<T: EventConsumer>(&mut self, duration: usize, events: &mut T) {
        for time in 0..duration {
            update_token_amounts(&mut self.participants, &self.params, time, events);
            manage_participants(
                &mut self.participants,
                time,
                events,
                &mut self.id_generator,
            );
            update_roles(&mut self.participants, &self.params, time, events);
        }
    }

    pub fn stake_fraction(&self) -> f64 {
        let mut total_bp_stake = 0f64;
        let mut total_cop_stake = 0f64;
        for p in self.participants.values() {
            match &p.role {
                Some(Role::BlockProducer) => total_bp_stake += p.num_tokens,
                Some(Role::ChunkOnlyProducer) => total_cop_stake += p.num_tokens,
                Some(Role::Delegator(id)) => match &self.participants.get(id).unwrap().role {
                    Some(Role::BlockProducer) => total_bp_stake += p.num_tokens,
                    Some(Role::ChunkOnlyProducer) => total_cop_stake += p.num_tokens,
                    None | Some(Role::Delegator(_)) => (),
                }
                None => (),
            }
        }
        total_cop_stake / total_bp_stake
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Participant {
    id: Id,
    num_tokens: f64,
    // BP, COP, or None if insufficient stake to be BP or COP
    role: Option<Role>,
    // actual stake change
    most_recent_stake_change: f64,
    // expected stake change if we switch roles
    expected_stake_change_on_switch: f64,
}

impl Participant {
    fn new(id_generator: &mut IdGenerator, num_tokens: f64) -> Self {
        Self {
            id: id_generator.next(),
            num_tokens,
            role: None,
            most_recent_stake_change: 0f64,
            expected_stake_change_on_switch: 0f64,
        }
    }

    fn split(self, id_generator: &mut IdGenerator) -> (Self, Self) {
        let new_id_1 = id_generator.next();
        let new_id_2 = id_generator.next();
        let mut template = Self {
            id: new_id_1,
            num_tokens: self.num_tokens / 2.0,
            role: self.role,
            most_recent_stake_change: self.most_recent_stake_change / 2.0,
            expected_stake_change_on_switch: self.expected_stake_change_on_switch / 2.0,
        };
        let p1 = template.clone();
        template.id = new_id_2;
        let p2 = template;
        (p1, p2)
    }
}

fn update_token_amounts<T: EventConsumer>(
    participants: &mut HashMap<Id, Participant>,
    params: &Params,
    time: usize,
    events: &mut T,
) {
    // effective_stake = num_tokens (owned) + delegated tokens
    let (effective_stakes, delegated_roles, total_bp_stake, total_cop_stake) = {
        let mut effective_stakes: HashMap<Id, f64> = HashMap::new();
        let mut delegated_roles: HashMap<Id, Option<Role>> = HashMap::new();
        let mut total_bp_stake = 0f64;
        let mut total_cop_stake = 0f64;
        for p in participants.values() {
            let stake = effective_stakes.entry(p.id).or_insert(0f64);
            *stake += p.num_tokens;
            match p.role {
                Some(Role::BlockProducer) => {
                    total_bp_stake += p.num_tokens;
                }
                Some(Role::ChunkOnlyProducer) => {
                    total_cop_stake += p.num_tokens;
                }
                Some(Role::Delegator(delegatee_id)) => {
                    let delegatee = participants.get(&delegatee_id).unwrap();
                    match &delegatee.role {
                        Some(Role::BlockProducer) => {
                            *effective_stakes.entry(delegatee_id).or_insert(0f64) += p.num_tokens;
                            total_bp_stake += p.num_tokens;
                            delegated_roles.insert(p.id, Some(Role::BlockProducer));
                        }
                        Some(Role::ChunkOnlyProducer) => {
                            *effective_stakes.entry(delegatee_id).or_insert(0f64) += p.num_tokens;
                            total_cop_stake += p.num_tokens;
                            delegated_roles.insert(p.id, Some(Role::ChunkOnlyProducer));
                        }
                        None | Some(Role::Delegator(_)) => (),
                    }
                }
                None => (),
            }
        }
        (
            effective_stakes,
            delegated_roles,
            total_bp_stake,
            total_cop_stake,
        )
    };

    let bp_cost = params.chunk_only_producer_cost * params.block_producer_cost_factor;
    let cop_reward_fraction = 1f64 - params.block_producer_reward_fraction;
    let bp_delegator_cost = 1f64 - params.block_producer_delegation_fee;
    let cop_delegator_cost = 1f64 - params.chunk_only_producer_delegation_fee;
    let mut bankrupt_participants: Vec<Id> = Vec::new();
    for p in participants.values_mut() {
        let change = match &p.role {
            None => 0f64, // bystanders gain nothing and lose nothing
            Some(Role::BlockProducer) => {
                let effective_stake = effective_stakes.get(&p.id).unwrap();
                let delegated_stake = effective_stake - p.num_tokens;
                let bp_profit =
                    (params.total_reward * params.block_producer_reward_fraction * effective_stake
                        / total_bp_stake)
                        - (params.total_reward
                            * params.block_producer_reward_fraction
                            * bp_delegator_cost
                            * delegated_stake
                            / total_bp_stake)
                        - bp_cost;
                // profit under the assumption only this participant switches from BP to COP
                let cop_profit = (params.total_reward * cop_reward_fraction * effective_stake
                    / (effective_stake + total_cop_stake))
                    - (params.total_reward
                        * cop_reward_fraction
                        * cop_delegator_cost
                        * delegated_stake
                        / (effective_stake + total_cop_stake))
                    - params.chunk_only_producer_cost;

                p.num_tokens += bp_profit;
                p.most_recent_stake_change = bp_profit;
                p.expected_stake_change_on_switch = cop_profit;

                bp_profit
            }
            Some(Role::ChunkOnlyProducer) => {
                let effective_stake = effective_stakes.get(&p.id).unwrap();
                let delegated_stake = effective_stake - p.num_tokens;
                let cop_profit = (params.total_reward * cop_reward_fraction * effective_stake
                    / total_cop_stake)
                    - (params.total_reward
                        * cop_reward_fraction
                        * cop_delegator_cost
                        * delegated_stake
                        / total_cop_stake)
                    - params.chunk_only_producer_cost;

                let bp_profit =
                    (params.total_reward * params.block_producer_reward_fraction * effective_stake
                        / (effective_stake + total_bp_stake))
                        - (params.total_reward
                            * params.block_producer_reward_fraction
                            * bp_delegator_cost
                            * delegated_stake
                            / (effective_stake + total_bp_stake))
                        - bp_cost;

                p.num_tokens += cop_profit;
                p.most_recent_stake_change = cop_profit;
                p.expected_stake_change_on_switch = bp_profit;

                cop_profit
            }
            Some(Role::Delegator(_)) => match delegated_roles.get(&p.id).unwrap() {
                Some(Role::BlockProducer) => {
                    let bp_reward =
                        params.total_reward * params.block_producer_reward_fraction * p.num_tokens
                            / total_bp_stake;
                    let bp_fee = bp_reward * params.block_producer_delegation_fee;
                    let bp_stake_change = bp_reward - bp_fee;

                    let cop_reward = params.total_reward * cop_reward_fraction * p.num_tokens
                        / (p.num_tokens + total_cop_stake);
                    let cop_fee = cop_reward * params.chunk_only_producer_delegation_fee;
                    let cop_stake_change = cop_reward - cop_fee;

                    p.num_tokens += bp_stake_change;
                    p.most_recent_stake_change = bp_stake_change;
                    p.expected_stake_change_on_switch = cop_stake_change;

                    bp_stake_change
                }
                Some(Role::ChunkOnlyProducer) => {
                    let cop_reward =
                        params.total_reward * cop_reward_fraction * p.num_tokens / total_cop_stake;
                    let cop_fee = cop_reward * params.chunk_only_producer_delegation_fee;
                    let cop_stake_change = cop_reward - cop_fee;

                    let bp_reward =
                        params.total_reward * params.block_producer_reward_fraction * p.num_tokens
                            / (p.num_tokens + total_bp_stake);
                    let bp_fee = bp_reward * params.block_producer_delegation_fee;
                    let bp_stake_change = bp_reward - bp_fee;

                    p.num_tokens += cop_stake_change;
                    p.most_recent_stake_change = cop_stake_change;
                    p.expected_stake_change_on_switch = bp_stake_change;

                    cop_stake_change
                }
                None | Some(Role::Delegator(_)) => 0f64,
            },
        };
        
        if change != 0f64 {
            if change > 0f64 || (change < 0f64 && p.num_tokens > 0f64) {
                events.push(Event {
                    time,
                    info: event::Info::StakeChange {
                        participant_id: p.id,
                        change_amount: change,
                    },
                })
            } else {
                events.push(Event {
                    time,
                    info: event::Info::ParticipantBankrupt {
                        participant_id: p.id,
                    },
                });
                bankrupt_participants.push(p.id);
            }
        }
    }

    for id in bankrupt_participants {
        participants.remove(&id);
    }
}

fn manage_participants<T: EventConsumer>(
    participants: &mut HashMap<Id, Participant>,
    time: usize,
    events: &mut T,
    id_generator: &mut IdGenerator,
) {
    // either introduce a new participant, split one participant into two, or merge two participants
    let mut rng = rand::thread_rng();
    let x: f64 = if participants.is_empty() {
        0.0
    } else {
        rng.gen()
    };
    if x < 0.333 {
        // introduce new participant
        let new_id = id_generator.next();
        let base_stake = if participants.is_empty() {
            100.0
        } else {
            let idx = rng.gen_range(0..participants.len());
            participants.values().skip(idx).next().unwrap().num_tokens
        };
        let modifier: f64 = 2.0 * rng.gen::<f64>();
        let p = Participant {
            id: new_id,
            num_tokens: modifier * base_stake,
            role: None,
            most_recent_stake_change: 0f64,
            expected_stake_change_on_switch: 0f64,
        };
        events.push(Event {
            time,
            info: event::Info::ParticipantCreated {
                participant_id: new_id,
                num_tokens: p.num_tokens,
            },
        });
        participants.insert(new_id, p);
    } else if x < 0.667 {
        // split one participant into two
        let idx = rng.gen_range(0..participants.len());
        let id = participants.values().skip(idx).next().unwrap().id;
        let original_particpiant = participants.remove(&id).unwrap();
        let (p1, p2) = original_particpiant.split(id_generator);
        events.push(Event {
            time,
            info: event::Info::ParticipantSplit {
                participant_id: id,
                new_participant_ids: (p1.id, p2.id),
            },
        });
        participants.insert(p1.id, p1);
        participants.insert(p2.id, p2);
    } else {
        // merge two participants
        let idx = rng.gen_range(0..participants.len());
        let id = participants.values().skip(idx).next().unwrap().id;
        let p1 = participants.remove(&id).unwrap();
        if let Some(p2_id) = participants
            .values()
            .filter(|p| p.role == p1.role)
            .next()
            .map(|p| p.id)
        {
            let p2 = participants.remove(&p2_id).unwrap();
            let new_id = id_generator.next();
            let p = Participant {
                id: new_id,
                num_tokens: p1.num_tokens + p2.num_tokens,
                role: p1.role,
                most_recent_stake_change: p1.most_recent_stake_change + p2.most_recent_stake_change,
                expected_stake_change_on_switch: p1.expected_stake_change_on_switch
                    + p2.expected_stake_change_on_switch,
            };
            events.push(Event {
                time,
                info: event::Info::ParticipantsMerged {
                    participant_ids: (p1.id, p2.id),
                    new_participant_id: new_id,
                },
            });
            participants.insert(new_id, p);
        }
    }
}

fn update_roles<T: EventConsumer>(
    participants: &mut HashMap<Id, Participant>,
    params: &Params,
    time: usize,
    events: &mut T,
) {
    let mut bp_proposals = Vec::with_capacity(params.num_block_producers);
    let mut cop_proposals = Vec::with_capacity(params.num_chunk_only_producers);

    let mut rng = rand::thread_rng();
    for p in participants.values() {
        // 80% chance to switch roles if the grass is greener on the other side, 20% otherwise
        let probability_to_switch =
            if p.most_recent_stake_change > p.expected_stake_change_on_switch {
                0.2f64
            } else {
                0.8f64
            };
        let x: f64 = rng.gen();
        match &p.role {
            None => {
                // Did not have a role in the last round; Randomly become BP or COP
                if rng.gen() {
                    bp_proposals.push((p.num_tokens, p.id));
                } else {
                    cop_proposals.push((p.num_tokens, p.id));
                }
            }
            Some(Role::BlockProducer) => {
                if x < probability_to_switch {
                    cop_proposals.push((p.num_tokens, p.id));
                } else {
                    bp_proposals.push((p.num_tokens, p.id));
                }
            }
            Some(Role::ChunkOnlyProducer) => {
                if x < probability_to_switch {
                    bp_proposals.push((p.num_tokens, p.id));
                } else {
                    cop_proposals.push((p.num_tokens, p.id));
                }
            }
            Some(Role::Delegator(id)) => match participants.get(id).and_then(|d| d.role) {
                Some(Role::BlockProducer) => {
                    if x < probability_to_switch {
                        cop_proposals.push((p.num_tokens, p.id));
                    } else {
                        bp_proposals.push((p.num_tokens, p.id));
                    }
                }
                Some(Role::ChunkOnlyProducer) => {
                    if x < probability_to_switch {
                        bp_proposals.push((p.num_tokens, p.id));
                    } else {
                        cop_proposals.push((p.num_tokens, p.id));
                    }
                }
                None | Some(Role::Delegator(_)) => {
                    if rng.gen() {
                        bp_proposals.push((p.num_tokens, p.id));
                    } else {
                        cop_proposals.push((p.num_tokens, p.id));
                    }
                }
            },
        }
    }

    bp_proposals.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap().reverse());
    cop_proposals.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap().reverse());

    let mut assign_role = |p: &mut Participant, new_role: Option<Role>| {
        if p.role != new_role {
            p.role = new_role;
            events.push(Event {
                time,
                info: event::Info::RoleChange {
                    participant_id: p.id,
                    new_role,
                },
            });
        }
    };

    // Top N proposals become BPs
    for (_, id) in bp_proposals.iter().take(params.num_block_producers) {
        let p = participants.get_mut(id).unwrap();
        assign_role(p, Some(Role::BlockProducer));
    }
    // Top M proposals become COPs
    for (_, id) in cop_proposals.iter().take(params.num_chunk_only_producers) {
        let p = participants.get_mut(id).unwrap();
        assign_role(p, Some(Role::ChunkOnlyProducer));
    }

    // All others delegate to someone in the same proposal group as them
    let mut i = 0;
    for (_, id) in bp_proposals.iter().skip(params.num_block_producers) {
        let (_, delegating_id) = bp_proposals[i];
        let p = participants.get_mut(id).unwrap();
        assign_role(p, Some(Role::Delegator(delegating_id)));
        i = (i + 1) % params.num_block_producers;
    }
    i = 0;
    for (_, id) in cop_proposals.iter().skip(params.num_chunk_only_producers) {
        let (_, delegating_id) = cop_proposals[i];
        let p = participants.get_mut(id).unwrap();
        assign_role(p, Some(Role::Delegator(delegating_id)));
        i = (i + 1) % params.num_chunk_only_producers;
    }
}
