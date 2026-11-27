#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a failure by panicking"
)]
#[global_allocator]
static ALLOCATOR: mcrs_test_alloc::CountingAllocator = mcrs_test_alloc::CountingAllocator;

mod colour_oracle;
mod colour_phases;
mod conformance_chain;
mod conformance_set;
mod conformance_tick;
mod determinism;
mod events;
mod probe;
mod random_ticks;
mod sequence;
mod sim_rules;
mod tick_model;
mod ticks;
