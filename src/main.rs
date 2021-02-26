extern crate rand;

mod event;
mod id;
mod role;
mod sim;

use crate::sim::{Params, Simulation};

fn main() {
    let params = Params {
        num_block_producers: 100,
        num_chunk_only_producers: 300,
        chunk_only_producer_cost: 10.0,
        block_producer_cost_factor: 2.5,
        total_reward: 1_000_000.0,
        block_producer_reward_fraction: 0.5,
        block_producer_delegation_fee: 0.15,
        chunk_only_producer_delegation_fee: 0.05,
    };

    let initial_stakes: Vec<f64> = (0..100)
        .flat_map(|i| {
            let x = 5000.0 - 2.0 * (i as f64);
            std::iter::repeat(x).take(i + 1)
        })
        .collect();

    let mut simulation = Simulation::new(&initial_stakes, params);
    let mut events = event::StatsAccumulator::default();
    simulation.run(100, &mut events);
    events
        .write_stats("/home/birchmd/rust/near-bp-sim/sim_stats.csv")
        .unwrap();

    println!("{:?}", simulation.stake_fraction());
}
