#[global_allocator]
static GLOBAL: mcrs_test_alloc::CountingAllocator = mcrs_test_alloc::CountingAllocator;

mod containers;
