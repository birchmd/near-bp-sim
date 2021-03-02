extern crate rand;

mod event;
mod id;
mod role;
mod sim;

use crate::sim::Simulation;
use std::path::Path;

fn run_with_params<S: AsRef<Path>, T: AsRef<Path>>(params_path: S, output_path: T) {
    let params_str = std::fs::read_to_string(params_path).unwrap();
    let params = serde_json::from_str(&params_str).unwrap();
    println!("{}", serde_json::to_string(&params).unwrap());
    let initial_stakes: Vec<f64> = (0..100)
        .flat_map(|i| {
            let x = 5000.0 - 2.0 * (i as f64);
            std::iter::repeat(x).take(i + 1)
        })
        .collect();

    let mut simulation = Simulation::new(&initial_stakes, params);
    let mut events = event::StatsAccumulator::default();
    simulation.run(40_000, &mut events);
    events.write_stats(output_path).unwrap();
    println!("{:?}", simulation.stake_fraction());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    run_with_params(&args[1], &args[2])
}
