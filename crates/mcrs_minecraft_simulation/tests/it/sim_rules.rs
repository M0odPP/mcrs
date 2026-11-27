use std::sync::Mutex;

use crate::probe::{
    A, A_OTHER, AIR, B, BLOCK_B, BLOCK_C, C, FLOWING, Host, NEEDS_SUPPORT, NEEDS_SUPPORT_WET,
    ProbeBlock, ProbeFacts, ProbeFluid, ProbeKinds, SOURCE, WATER, at, section,
};
use mcrs_minecraft_core::Direction;
use mcrs_minecraft_random::legacy::LegacyRandom;
use mcrs_minecraft_simulation::{
    BlockBehaviour, BlockFacts, BlockShard, BlockUpdateFlags, Cx, DEFAULT_RANDOM_TICK_SPEED,
    DEFAULT_UPDATE_LIMIT, Drops, Env, Footprints, GameTick, KernelConfig, PartitionKind, Phase,
    PhaseInput, PistonMove, RandomHit, RandomPicks, RandomTickTable, RandomTicks, ScheduledTick,
    SemanticOrder, Serial, ShardId, SimulationVolume, StepAt, StepOutput, StreamSeed, SubTick,
    TickKey, TickKind, TickPriority, TickingPositions, TickingSectionMut, draw_random_picks,
    random_step, step,
};
use mcrs_minecraft_volume::{
    BlockVolume, BlockVolumeMut, PaletteBuffer, SectionBlocksMut, SparseVolume,
};
use mcrs_test_alloc::allocations;
use mcrs_voxel::{BlockPos, BlockStateId, ColumnPos, LocalPos, SectionPos};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FluidQueryState {
    Idle,
    Scheduled,
    Ticking,
    TickingAndScheduled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RecordedEvent {
    ShapeChanged(i32),
    FluidTick(BlockPos, FluidQueryState),
    BlockIdle(BlockPos),
    BlockTicking(BlockPos),
    Removed(BlockPos, PistonMove),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Rules,
    Steady,
}

struct SimulationBehaviour {
    mode: Mode,
    recorded: Mutex<Vec<RecordedEvent>>,
}

impl SimulationBehaviour {
    fn push(&self, event: RecordedEvent) {
        self.recorded.lock().expect("lock").push(event);
    }

    fn record_fluid<V: SimulationVolume, F: BlockFacts<ProbeKinds>>(
        &self,
        cx: &mut Cx<'_, ProbeKinds, V, F, Self>,
        position: BlockPos,
        fluid: ProbeFluid,
    ) {
        let state = match (
            cx.will_fluid_tick_this_tick(position, fluid),
            cx.has_scheduled_fluid_tick(position, fluid),
        ) {
            (true, true) => FluidQueryState::TickingAndScheduled,
            (true, false) => FluidQueryState::Ticking,
            (false, true) => FluidQueryState::Scheduled,
            (false, false) => FluidQueryState::Idle,
        };
        self.push(RecordedEvent::FluidTick(position, state));
    }
}

impl BlockBehaviour<ProbeKinds> for SimulationBehaviour {
    fn block_tick<V: SimulationVolume, F: BlockFacts<ProbeKinds>>(
        &self,
        cx: &mut Cx<'_, ProbeKinds, V, F, Self>,
        position: BlockPos,
        state: BlockStateId,
    ) {
        let block = cx.facts().block_of(state);
        if self.mode == Mode::Steady {
            cx.schedule_block_tick(position, block, 1, TickPriority::Normal);
            cx.update_neighbour(at(position.x + 1, position.y, position.z), block);
        } else if state == B {
            let above = at(position.x, position.y + 1, position.z);
            let flags = BlockUpdateFlags::CLIENTS;
            cx.update_shape(above, Direction::Down, AIR, flags, DEFAULT_UPDATE_LIMIT);
        } else if state == C {
            cx.remove_block(position, PistonMove::No);
            cx.remove_block(at(position.x + 1, position.y, position.z), PistonMove::Yes);
            let target = at(position.x + 2, position.y, position.z);
            cx.destroy_block(target, Drops::Suppress, DEFAULT_UPDATE_LIMIT);
        } else {
            self.record_fluid(cx, at(1, 1, 1), WATER);
        }
    }

    fn fluid_tick<V: SimulationVolume, F: BlockFacts<ProbeKinds>>(
        &self,
        cx: &mut Cx<'_, ProbeKinds, V, F, Self>,
        position: BlockPos,
        _state: BlockStateId,
        fluid: ProbeFluid,
    ) {
        if self.mode == Mode::Steady {
            cx.schedule_fluid_tick(position, fluid, 1, TickPriority::Normal);
            cx.queue_block_event(at(2, 2, 2), BLOCK_B, 2, 3);
            return;
        }
        let state = cx.state(position).expect("position in volume");
        let event = if cx.will_tick_this_tick(position, cx.facts().block_of(state)) {
            RecordedEvent::BlockTicking(position)
        } else {
            RecordedEvent::BlockIdle(position)
        };
        self.push(event);
        for coordinate_x in 1..=3 {
            self.record_fluid(cx, at(coordinate_x, 1, 1), fluid);
        }
    }

    fn random_tick<V: SimulationVolume, F: BlockFacts<ProbeKinds>>(
        &self,
        cx: &mut Cx<'_, ProbeKinds, V, F, Self>,
        _position: BlockPos,
        _state: BlockStateId,
        _random: &mut LegacyRandom,
    ) {
        if self.mode == Mode::Steady {
            let toggle = at(2, 3, 2);
            let next = [A, A_OTHER][usize::from(cx.state(toggle) == Ok(A))];
            let flags = BlockUpdateFlags::CLIENTS;
            cx.set(toggle, next, flags, DEFAULT_UPDATE_LIMIT);
        }
    }

    fn block_event<V: SimulationVolume, F: BlockFacts<ProbeKinds>>(
        &self,
        _cx: &mut Cx<'_, ProbeKinds, V, F, Self>,
        _position: BlockPos,
        _state: BlockStateId,
        _event_id: i32,
        _event_data: i32,
    ) -> bool {
        self.mode == Mode::Steady
    }

    fn removed<V: SimulationVolume, F: BlockFacts<ProbeKinds>>(
        &self,
        _cx: &mut Cx<'_, ProbeKinds, V, F, Self>,
        position: BlockPos,
        _old: BlockStateId,
        moved_by_piston: bool,
    ) {
        let motion = if moved_by_piston {
            PistonMove::Yes
        } else {
            PistonMove::No
        };
        self.push(RecordedEvent::Removed(position, motion));
    }

    fn update_shape<V: SimulationVolume, F: BlockFacts<ProbeKinds>>(
        &self,
        _cx: &mut Cx<'_, ProbeKinds, V, F, Self>,
        position: BlockPos,
        state: BlockStateId,
        _from: Direction,
        _neighbour: BlockPos,
        _neighbour_state: BlockStateId,
    ) -> BlockStateId {
        if state == NEEDS_SUPPORT {
            self.push(RecordedEvent::ShapeChanged(position.y));
            return AIR;
        }
        state
    }
}

fn scheduled<T: TickKind>(
    position: BlockPos,
    kind: T,
    due: i64,
    sequence: u32,
) -> ScheduledTick<T> {
    let key = TickKey {
        due: GameTick(due),
        priority: TickPriority::Normal,
        sequence: SubTick::runtime(GameTick(0), sequence),
    };
    ScheduledTick {
        pos: position,
        kind,
        key,
    }
}

fn env_for<'a>(
    behaviour: &'a SimulationBehaviour,
    config: &'a KernelConfig,
) -> Env<'a, ProbeFacts, SimulationBehaviour, SemanticOrder> {
    Env {
        partition: ShardId(0),
        facts: &ProbeFacts,
        behaviour,
        policy: &SemanticOrder,
        kind: PartitionKind::Shard,
        config,
    }
}

struct Runner {
    shard: BlockShard<ProbeKinds>,
    host: Host,
    behaviour: SimulationBehaviour,
    output: StepOutput<ProbeKinds>,
}

impl Runner {
    fn new(host: Host, mode: Mode) -> Self {
        let behaviour = SimulationBehaviour {
            mode,
            recorded: Mutex::default(),
        };
        Self {
            shard: BlockShard::new(),
            host,
            behaviour,
            output: StepOutput::default(),
        }
    }

    fn queue_block(&mut self, position: BlockPos, block: ProbeBlock, due: i64, sequence: u32) {
        let ticks = self.shard.block_ticks_mut();
        ticks.schedule(scheduled(position, block, due, sequence));
    }

    fn queue_fluid(&mut self, position: BlockPos, fluid: ProbeFluid, due: i64, sequence: u32) {
        let ticks = self.shard.fluid_ticks_mut();
        ticks.schedule(scheduled(position, fluid, due, sequence));
    }

    fn tick(&mut self, now: i64, phases: &[Phase]) -> usize {
        let config = KernelConfig::default();
        let env = env_for(&self.behaviour, &config);
        let (shard, host, output) = (&mut self.shard, &mut self.host, &mut self.output);
        let mut delivered = 0;
        for &phase in phases {
            output.clear();
            let input = PhaseInput {
                inbound: &[],
                ledgers: &[],
            };
            let at_step = StepAt {
                now: GameTick(now),
                phase,
            };
            step(shard, at_step, host, &env, input, output).expect("step");
            delivered += output.delivered.len();
        }
        delivered
    }

    fn events(&self) -> Vec<RecordedEvent> {
        self.behaviour.recorded.lock().expect("lock").clone()
    }
}

#[test]
fn sim_sch_010_and_tick_conversions() {
    use FluidQueryState::{Idle, Scheduled, Ticking};
    use Phase::{BlockTicks, FluidTicks};
    use RecordedEvent::{BlockIdle, FluidTick};
    let mut host = Host::new();
    host.insert(section(0, 0, 0), FLOWING);
    let (first, second, third) = (at(1, 1, 1), at(2, 1, 1), at(3, 1, 1));
    host.put(first, SOURCE);
    let mut runner = Runner::new(host, Mode::Rules);
    runner.queue_block(first, ProbeFacts.block_of(SOURCE), 10, 0);
    runner.queue_fluid(first, WATER, 10, 1);
    runner.queue_fluid(second, WATER, 10, 2);
    runner.queue_fluid(third, WATER, 11, 3);
    runner.tick(10, &[BlockTicks, FluidTicks]);
    let events = runner.events();
    assert_eq!(events[0], FluidTick(first, Scheduled));
    assert_eq!(events[1], BlockIdle(first));
    assert_eq!(events[2], FluidTick(first, Idle));
    assert_eq!(events[3], FluidTick(second, Ticking));
    assert_eq!(events[4], FluidTick(third, Scheduled));
    assert_eq!(events[5], BlockIdle(second));
    assert_eq!(events[7], FluidTick(second, Idle));

    let mut tick = scheduled(at(-5, 64, 17), 42u16, 993, 0);
    tick.key.priority = TickPriority::High;
    tick.key.sequence = SubTick::restored(2, 5).expect("sequence");
    let saved = tick.to_saved(GameTick(1_000));
    assert_eq!(saved.delay, -7);
    let restored = saved.into_scheduled(GameTick(5_000));
    assert_eq!(restored.key.due, GameTick(4_993));
    assert_eq!(restored.key.priority, TickPriority::High);
    let overdue = scheduled(at(0, 0, 0), 1u16, i64::MAX, 0);
    assert_eq!(overdue.delay_at(GameTick(-1_000)), i32::MAX);
}

#[test]
fn sim_set_004_and_shp_001_remove_block_and_direct_shape_cascade() {
    assert_eq!(DEFAULT_UPDATE_LIMIT, 512);
    let mut host = Host::new();
    host.insert(section(0, 0, 0), A);
    let (origin, wet, destroyed) = (at(2, 2, 2), at(3, 2, 2), at(4, 2, 2));
    host.put(origin, C);
    host.put(wet, NEEDS_SUPPORT_WET);
    host.put(destroyed, B);
    host.put(at(5, 0, 5), B);
    host.put(at(5, 1, 5), NEEDS_SUPPORT);
    host.put(at(5, 2, 5), NEEDS_SUPPORT);
    let mut runner = Runner::new(host, Mode::Rules);
    runner.queue_block(origin, BLOCK_C, 1, 0);
    runner.queue_block(at(5, 0, 5), BLOCK_B, 1, 1);
    runner.tick(1, &[Phase::BlockTicks]);
    assert_eq!(runner.host.state(origin), AIR);
    assert_eq!(runner.host.state(wet), SOURCE);
    assert_eq!(runner.host.state(destroyed), AIR);
    assert_eq!(runner.host.state(at(5, 1, 5)), AIR);
    assert_eq!(runner.host.state(at(5, 2, 5)), AIR);
    assert_eq!(runner.output.destroyed.len(), 3);
    assert!(!runner.output.destroyed[0].drops, "suppress drops");
    use PistonMove::{No, Yes};
    use RecordedEvent::{Removed, ShapeChanged};
    let expected = [
        Removed(origin, No),
        Removed(wet, Yes),
        Removed(destroyed, No),
        ShapeChanged(1),
        Removed(at(5, 1, 5), No),
        ShapeChanged(2),
        Removed(at(5, 2, 5), No),
    ];
    assert_eq!(runner.events(), expected);
}

#[test]
fn steady_state_step_and_random_step_perform_zero_heap_allocations() {
    use Phase::{BlockEvents, BlockTicks, FluidTicks, Ingress};
    let section_pos = section(0, 0, 0);
    let mut host = Host::new();
    host.insert(section_pos, B);
    host.put(at(4, 2, 4), FLOWING);
    host.put(at(1, 3, 1), A);
    host.put(at(1, 4, 1), A_OTHER);
    host.put(at(2, 3, 2), A);
    let table = RandomTickTable::new([B], |_| RandomTicks::BLOCK);
    let (mut ticking, mut buffer) = (TickingPositions::default(), PaletteBuffer::default());
    let view = host.section(section_pos).expect("section");
    ticking.rebuild(&view, &table, &mut buffer);
    let mut runner = Runner::new(host, Mode::Steady);
    runner.queue_block(at(2, 2, 2), BLOCK_B, 1, 0);
    runner.queue_fluid(at(4, 2, 4), WATER, 1, 1);
    let config = KernelConfig::default();
    let footprints = Footprints::new([AIR, A, A_OTHER, B, FLOWING], |_| 1);
    let seed = StreamSeed::new(42, "minecraft:overworld");
    let columns = [ColumnPos::new(0, 0)];
    let (mut picks, mut hits) = (Vec::with_capacity(64), Vec::with_capacity(64));
    let (mut before, mut delivered_count, mut write_count) = (0usize, 0usize, 0usize);
    let speed = DEFAULT_RANDOM_TICK_SPEED;
    let phases = [Ingress, BlockTicks, FluidTicks, BlockEvents];
    for tick in 1..=500i64 {
        if tick == 301 {
            before = allocations();
        }
        let now = GameTick(tick);
        let delivered = runner.tick(tick, &phases);
        picks.clear();
        hits.clear();
        let (streams, count) = (seed.sections(now), ticking.len());
        draw_random_picks(section_pos, count, speed, streams, &mut picks);
        for pick in picks.drain(..) {
            hits.extend(RandomHit::of(section_pos, ticking.as_slice(), pick));
        }
        let random_picks = RandomPicks {
            columns: &columns,
            hits: &hits,
            speed,
            table: &table,
            footprints: &footprints,
            seed,
        };
        let env = env_for(&runner.behaviour, &config);
        let (shard, host, output) = (&mut runner.shard, &mut runner.host, &mut runner.output);
        output.clear();
        random_step(shard, now, host, &env, random_picks, output, &Serial);
        if tick >= 301 {
            delivered_count += delivered;
            write_count += runner.output.writes.len();
        }
    }
    assert_eq!(allocations(), before, "steady-state must not allocate");
    assert!(delivered_count > 0 && write_count > 0, "steady events");
}

#[test]
fn ticking_positions_track_counts_empty_draws_and_clear_zero_allocations() {
    let table = RandomTickTable::new([B, FLOWING, SOURCE], |state| match state {
        value if value == B => RandomTicks::BLOCK,
        value if value == FLOWING => RandomTicks::FLUID,
        value if value == SOURCE => RandomTicks::BLOCK | RandomTicks::FLUID,
        _ => RandomTicks::empty(),
    });
    let section_pos = SectionPos::new(0, 0, 0);
    let mut volume = SparseVolume::unbounded();
    volume.insert(section_pos, AIR);
    let (mut ticking, mut buffer) = (TickingPositions::default(), PaletteBuffer::default());
    let view = volume.section(section_pos).expect("section");
    ticking.rebuild(&view, &table, &mut buffer);
    assert!(ticking.is_empty(), "empty air section starts empty");
    let blocks = volume.section_mut(section_pos).expect("section");
    TickingSectionMut::new(blocks, &mut ticking, &table).fill(B);
    assert_eq!(ticking.len(), 4096);
    assert_eq!((ticking.block_count(), ticking.fluid_count()), (4096, 0));
    let blocks = volume.section_mut(section_pos).expect("section");
    let mut tracked = TickingSectionMut::new(blocks, &mut ticking, &table);
    tracked.set(LocalPos::new(1, 2, 3), SOURCE);
    tracked.set(LocalPos::new(4, 5, 6), FLOWING);
    assert_eq!((ticking.block_count(), ticking.fluid_count()), (4095, 2));
    let before = allocations();
    let blocks = volume.section_mut(section_pos).expect("section");
    TickingSectionMut::new(blocks, &mut ticking, &table).fill(AIR);
    assert_eq!(allocations(), before, "clear must not allocate");
    assert_eq!((ticking.block_count(), ticking.fluid_count()), (0, 0));
    let mut picks = Vec::new();
    let streams = StreamSeed::new(7, "minecraft:overworld").sections(GameTick(12));
    draw_random_picks(section_pos, 0, 1_000, streams, &mut picks);
    assert!(picks.is_empty() && picks.capacity() == 0, "empty");
}
