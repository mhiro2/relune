//! Wall-clock benchmark for laying out large schemas.
//!
//! Run with `make bench` (or `cargo bench -p relune-layout`). Sizes and the
//! number of timed runs can be overridden with `RELUNE_BENCH_TABLES`
//! (comma-separated) and `RELUNE_BENCH_RUNS`.

#[path = "../tests/support/mod.rs"]
mod support;

use std::hint::black_box;
use std::time::{Duration, Instant};

use relune_core::{GroupingSpec, GroupingStrategy, LayoutAlgorithm};
use relune_layout::{LayoutConfig, LayoutRequest, build_layout_with_config};

const DEFAULT_TABLES: [usize; 3] = [500, 1000, 2000];
const DEFAULT_RUNS: usize = 3;
const SEED: u64 = 0x5eed;

fn env_list(name: &str) -> Option<Vec<usize>> {
    let value = std::env::var(name).ok()?;
    value
        .split(',')
        .map(|part| part.trim().parse().ok())
        .collect()
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn main() {
    let sizes = env_list("RELUNE_BENCH_TABLES").unwrap_or_else(|| DEFAULT_TABLES.to_vec());
    let runs = env_list("RELUNE_BENCH_RUNS")
        .and_then(|runs| runs.first().copied())
        .unwrap_or(DEFAULT_RUNS)
        .max(1);
    let scenarios = [
        (
            "hierarchical",
            LayoutAlgorithm::Hierarchical,
            GroupingStrategy::None,
        ),
        (
            "hierarchical+schema",
            LayoutAlgorithm::Hierarchical,
            GroupingStrategy::BySchema,
        ),
        (
            "force",
            LayoutAlgorithm::ForceDirected,
            GroupingStrategy::None,
        ),
    ];

    println!(
        "{:<20} {:>7} {:>7} {:>11} {:>17}",
        "scenario", "tables", "edges", "median", "canvas"
    );
    for &tables in &sizes {
        let schema = support::synthetic_schema(tables, SEED);
        for (label, mode, strategy) in scenarios {
            let request = LayoutRequest {
                grouping: GroupingSpec { strategy },
                ..LayoutRequest::default()
            };
            let config = LayoutConfig {
                mode,
                ..LayoutConfig::default()
            };
            let mut samples = Vec::with_capacity(runs);
            let mut graph = None;
            for _ in 0..runs {
                let start = Instant::now();
                let result = build_layout_with_config(black_box(&schema), &request, &config)
                    .expect("synthetic schemas lay out");
                samples.push(start.elapsed());
                graph = Some(result);
            }
            let graph = graph.expect("at least one run");
            println!(
                "{label:<20} {tables:>7} {:>7} {:>9.1}ms {:>8.0} x {:<7.0}",
                graph.edges.len(),
                median(samples).as_secs_f64() * 1000.0,
                graph.width,
                graph.height
            );
        }
    }
}
