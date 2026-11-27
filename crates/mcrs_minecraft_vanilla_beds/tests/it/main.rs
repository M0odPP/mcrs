#![expect(clippy::expect_used, reason = "integration tests")]

use bevy_app::{App, FixedUpdate};
use bevy_ecs::message::{Message, MessageReader, MessageWriter, Messages};
use bevy_ecs::prelude::{Entity, IntoScheduleConfigs};
use bevy_ecs::system::SystemState;
use glam::{DVec3, Vec3};
use mcrs_minecraft_block::block_keys::Block;
use mcrs_minecraft_block::definition::Blocks;
use mcrs_minecraft_core::Direction;
use mcrs_minecraft_dimension::{Dimension as DimensionKey, DimensionType, dimension_keys};
use mcrs_minecraft_engine::ShiftKey;
use mcrs_minecraft_engine::bus::{OutboundPlayerPacket, PacketPayload};
use mcrs_minecraft_engine::player_state::{Ints, PlayerFileState};
use mcrs_minecraft_engine::use_item_on_request::UseItemOnRequest;
use mcrs_minecraft_engine_item::{ItemPlugin, Selected, Slots, StackCommandsExt};
use mcrs_minecraft_engine_registries::test_corpus;
use mcrs_minecraft_engine_world::block_update::{BlockPlaced, BlockSetRequest, remove_block};
use mcrs_minecraft_engine_world::entity::physics::{Rotation, Transform};
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::palette;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_engine_world::tick::TickSet;
use mcrs_minecraft_engine_world::voxel_update::{self, CellBlockWrite};
use mcrs_minecraft_engine_world::world::dimension::{
    Dimension, DimensionPlugin, DimensionTypeConfig, DimensionTypeId, InDimension,
};
use mcrs_minecraft_engine_world::world::storage::section::SectionIndex;
use mcrs_minecraft_engine_world::world::storage::view::{WorldView, WorldViewMut};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_game_registries::registry_load::test_registries;
use mcrs_minecraft_item::Items;
use mcrs_minecraft_item_stack::{Count, Kind};
use mcrs_minecraft_pose::Pose;
use mcrs_minecraft_protocol::VarInt;
use mcrs_minecraft_protocol_game::Hand;
use mcrs_minecraft_registries::CurrentRegistries;
use mcrs_minecraft_resource::{ResourceKey, ResourceKeyBuf};
use mcrs_minecraft_text::{RawText, Text};
use mcrs_minecraft_vanilla_beds::{BedBlockTable, BedExplosionRequest, VanillaBedsPlugin};
use mcrs_minecraft_volume::{BlockVolume, BlockVolumeMut};
use mcrs_voxel::{BlockPos, BlockStateId, SectionPos};

const fn at(x: i32, y: i32, z: i32) -> BlockPos {
    BlockPos::new(x, y, z)
}

fn apply_writes(
    mut requests: MessageReader<BlockSetRequest>,
    mut world: WorldViewMut,
    mut placed: MessageWriter<BlockPlaced>,
) {
    for request in requests.read() {
        let cell = CellBlockWrite {
            pos: request.pos,
            new_state: request.new_state,
            flags: request.flags,
            recursion_left: request.recursion_left,
        };
        voxel_update::apply_write(&mut world, &mut placed, request.dimension, cell);
    }
}

struct Harness {
    app: App,
    dimension: Entity,
    sequence: i32,
}

impl Harness {
    fn new(key: ResourceKey<'static, DimensionKey>) -> Self {
        let mut app = App::new();
        app.add_plugins((DimensionPlugin, ItemPlugin::default(), VanillaBedsPlugin));
        app.add_systems(FixedUpdate, apply_writes.in_set(TickSet::WorldWrite));
        let registries = test_registries().clone();
        let (blocks, items) = test_corpus();
        let entries = registries.entries::<DimensionType>();
        let types = entries.expect("dimension types missing");
        let found = types.by_name(key.location());
        let type_id = found.expect("dimension type missing");
        app.insert_resource(CurrentRegistries::new(registries));
        app.insert_resource(blocks.clone());
        app.insert_resource(items.clone());
        let chunk = app.world_mut().spawn_empty().id();
        let section = SectionPos::new(0, 4, 0);
        let mut index = SectionIndex::new();
        index.insert(section, chunk);
        index.land(section, chunk, palette::air_section(), None, false);
        let bundle = (
            Dimension,
            ResourceKeyBuf::from(key),
            DimensionTypeId(type_id),
            index,
            DimensionTypeConfig::new(-64, 384),
        );
        let dimension = app.world_mut().spawn(bundle).id();
        let mut harness = Self {
            app,
            dimension,
            sequence: 0,
        };
        harness.tick(1);
        harness
    }

    fn tick(&mut self, count: usize) {
        for _ in 0..count {
            self.app.world_mut().run_schedule(FixedUpdate);
        }
    }

    fn drain<M: Message>(&mut self) -> Vec<M> {
        let mut messages = self.app.world_mut().resource_mut::<Messages<M>>();
        messages.drain().collect()
    }

    fn player(&mut self, session: u64, x: f64, z: f64) -> Entity {
        let transform = Transform::from_translation(DVec3::new(x, 65.0, z))
            .with_rotation(Rotation::new(180.0, 0.0));
        let bundle = (
            Player,
            Owner(PlayerSession(session)),
            InDimension(self.dimension),
            transform,
            Pose::Standing,
            PlayerGameMode(GameMode::Survival),
            ShiftKey(false),
            Slots::new(9),
            Selected(0),
        );
        self.app.world_mut().spawn(bundle).id()
    }

    fn set(&mut self, block_pos: BlockPos, state: BlockStateId) {
        let world = self.app.world_mut();
        let mut access: SystemState<WorldViewMut<'_, '_>> = SystemState::new(world);
        let mut view = access.get_mut(world).expect("world view missing");
        let entry = view.get_mut(self.dimension);
        let mut entry = entry.expect("dimension entity missing");
        let result = entry.set_block(block_pos, state);
        result.expect("failed to set block");
    }

    fn get(&mut self, block_pos: BlockPos) -> BlockStateId {
        let mut access: SystemState<WorldView<'_, '_>> = SystemState::new(self.app.world_mut());
        let world = access.get(self.app.world()).expect("world view missing");
        let found = world.get(self.dimension);
        let dimension = found.expect("dimension entity missing");
        dimension.block(block_pos).unwrap_or(BlockStateId::AIR)
    }

    fn is_bed(&mut self, block_pos: BlockPos) -> bool {
        let state = self.get(block_pos);
        let table = self.app.world().resource::<BedBlockTable>();
        table.get(state).is_some()
    }

    fn place_bed(&mut self, bed: Block, foot: BlockPos, head: BlockPos) {
        let blocks = self.app.world().resource::<Blocks>();
        let entry = blocks.entry(bed.id()).expect("bed block missing");
        let with_part = |part| {
            let base = entry.with_text(entry.default_state_id, "part", part)?;
            entry.with_text(base, "facing", "north")
        };
        let foot_state = with_part("foot").expect("foot state missing");
        let head_state = with_part("head").expect("head state missing");
        self.set(foot, foot_state);
        self.set(head, head_state);
    }

    fn click(&mut self, entity: Entity, block_pos: BlockPos) {
        self.sequence += 1;
        let request = UseItemOnRequest {
            entity,
            hand: Hand::Main,
            block_pos,
            face: Direction::Up,
            cursor: Vec3::splat(0.5),
            sequence: VarInt(self.sequence),
        };
        self.app.world_mut().write_message(request);
        self.tick(1);
    }

    fn spawn_chat(&mut self) -> bool {
        let text: Text = Text::translate("block.minecraft.set_spawn", Vec::new());
        let raw = RawText::from_text(&text).expect("failed to encode text");
        let bytes = raw.as_bytes();
        let packets = self.drain::<OutboundPlayerPacket>();
        packets.into_iter().any(|packet| match packet.data {
            PacketPayload::SystemChat(chat) => !chat.overlay && chat.content.as_bytes() == bytes,
            _ => false,
        })
    }

    fn respawn(&self, player: Entity) -> Option<Ints<3>> {
        let file = self.app.world().get::<PlayerFileState>(player)?;
        file.respawn.as_ref().map(|entry| entry.pos)
    }
}

#[test]
fn two_part_placement_break_and_obstructed_head() {
    let mut harness = Harness::new(dimension_keys::OVERWORLD);
    let blocks = harness.app.world().resource::<Blocks>();
    let stone = blocks.default_state_of(Block::Stone.id());
    harness.set(at(2, 64, 4), stone);
    harness.set(at(5, 64, 4), stone);
    let player = harness.player(1, 2.5, 6.0);
    let items = harness.app.world().resource::<Items>();
    let found = items.id_of("minecraft:red_bed");
    let item = found.expect("red bed item missing");
    let count = Count::new(2).expect("count must be positive");
    let world = harness.app.world_mut();
    let stack = world.spawn((Kind(item.number()), count)).id();
    world.commands().entity(stack).put_at(player, 0);
    world.flush();
    harness.click(player, at(2, 64, 4));
    assert!(harness.is_bed(at(2, 65, 4)), "foot half should be placed");
    assert!(harness.is_bed(at(2, 65, 3)), "head half should be placed");
    let world = harness.app.world();
    assert_eq!(world.get::<Count>(stack).copied().map(Count::get), Some(1));
    let remove = remove_block(harness.dimension, at(2, 65, 4));
    harness.app.world_mut().write_message(remove);
    harness.tick(2);
    assert_eq!(harness.get(at(2, 65, 3)), BlockStateId::AIR);
    harness.set(at(5, 65, 3), stone);
    harness.click(player, at(5, 64, 4));
    assert_eq!(harness.get(at(5, 65, 4)), BlockStateId::AIR);
    let world = harness.app.world();
    assert_eq!(world.get::<Count>(stack).copied().map(Count::get), Some(1));
}

#[test]
fn daytime_spawn_point_and_straw_bed_rule() {
    let mut harness = Harness::new(dimension_keys::OVERWORLD);
    let player = harness.player(1, 3.5, 4.0);
    harness.place_bed(Block::RedBed, at(2, 65, 4), at(2, 65, 3));
    harness.place_bed(Block::StrawBed, at(4, 65, 4), at(4, 65, 3));
    harness.click(player, at(4, 65, 3));
    assert_eq!(harness.respawn(player), None);
    assert!(!harness.spawn_chat(), "straw bed must not set spawn");
    harness.click(player, at(2, 65, 3));
    assert_eq!(harness.respawn(player), Some(Ints([2, 65, 3])));
    assert!(harness.spawn_chat(), "red bed must set spawn");
    harness.click(player, at(2, 65, 3));
    assert!(!harness.spawn_chat(), "unchanged spawn must not repeat");
    harness.click(player, at(4, 65, 3));
    assert_eq!(harness.respawn(player), Some(Ints([2, 65, 3])));
}

#[test]
fn nether_and_end_bed_explosion() {
    for key in [dimension_keys::THE_NETHER, dimension_keys::THE_END] {
        let mut harness = Harness::new(key);
        let player = harness.player(1, 2.5, 5.0);
        harness.place_bed(Block::RedBed, at(2, 65, 4), at(2, 65, 3));
        harness.click(player, at(2, 65, 4));
        assert_eq!(harness.get(at(2, 65, 4)), BlockStateId::AIR);
        assert_eq!(harness.get(at(2, 65, 3)), BlockStateId::AIR);
        let explosions = harness.drain::<BedExplosionRequest>();
        assert_eq!(explosions.len(), 1);
        let item = &explosions[0];
        let valid = item.dimension == harness.dimension
            && item.center.abs_diff_eq(DVec3::new(2.5, 65.5, 3.5), 1e-6)
            && (item.power - 5.0).abs() < 1e-6
            && item.fire;
        assert!(valid, "explosion request should match bed and spec");
    }
}
