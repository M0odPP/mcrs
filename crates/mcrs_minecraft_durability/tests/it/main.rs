mod durability;

use mcrs_test_alloc::CountingAllocator;

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;
