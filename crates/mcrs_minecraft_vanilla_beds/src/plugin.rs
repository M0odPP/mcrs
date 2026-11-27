use bevy_app::{App, FixedUpdate, Plugin};
use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::prelude::{
    Added, Changed, Commands, DetectChanges, Entity, IntoScheduleConfigs, Local, Message,
    MessageReader, MessageWriter, Or, Query, Res, ResMut, Resource, SystemCondition, With,
};
use bevy_ecs::query::QueryData;
use bevy_ecs::schedule::common_conditions::{
    on_message, resource_exists, resource_exists_and_changed,
};
use bevy_ecs::system::SystemParam;
use glam::DVec3;
use mcrs_minecraft_beds::{
    BED_EXPLOSION_SPEC, BED_TOP_OFFSET, BedFacing, BedPair, BedPart, BedRuleCondition, BedRuleSpec,
    BedState, BedUseOutcome, EXPLODING_BED_RULE, OVERWORLD_BED_RULE, OVERWORLD_STRAW_BED_RULE,
    SleepCheckInput, SleepRejection, WorldPoint, bed_in_reach, can_condition_pass,
    evaluate_bed_use, explosion_origin,
};
use mcrs_minecraft_block::block_keys::Block;
use mcrs_minecraft_block::block_tag_keys;
use mcrs_minecraft_block::definition::{BlockEntry, Blocks};
use mcrs_minecraft_core::Direction;
use mcrs_minecraft_dimension::{Dimension as DimensionKey, dimension_keys};
use mcrs_minecraft_dimension_environment::environment::{DimensionAttributes, EnvironmentContext};
use mcrs_minecraft_engine::block_simulation::BlockSimulationSet;
use mcrs_minecraft_engine::bus::{OutboundPlayerPacket, PacketPayload, to};
use mcrs_minecraft_engine::player_state::{Ints, PlayerFileState, Respawn};
use mcrs_minecraft_engine::swing::SwingPlayer;
use mcrs_minecraft_engine::use_item_on_request::UseItemOnRequest;
use mcrs_minecraft_engine::{BlockBreakSet, PlayerBody, ShiftKey, eye_position, in_reach, reach};
use mcrs_minecraft_engine_item::{Selected, Slots, StackCommandsExt};
use mcrs_minecraft_engine_world::block_update::{BlockPlaced, BlockSetRequest, remove_block};
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::entity::player::Player;
use mcrs_minecraft_engine_world::session::{Owner, PlayerSession};
use mcrs_minecraft_engine_world::tick::TickSet;
use mcrs_minecraft_engine_world::world::dimension::{Dimension, DimensionTypeId, InDimension};
use mcrs_minecraft_engine_world::world::storage::section::SectionIndex;
use mcrs_minecraft_engine_world::world::storage::view::{DimensionView, WorldView};
use mcrs_minecraft_environment::environment_attribute_keys::EnvironmentAttribute;
use mcrs_minecraft_environment::{AttributeValue, BedRuleCondition as EnvBedCondition};
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_inventory_player::OFF_HAND;
use mcrs_minecraft_item::Items;
use mcrs_minecraft_item_stack::{Count, Kind};
use mcrs_minecraft_physics::Aabb;
use mcrs_minecraft_protocol_game::block::BlockStateNumber;
use mcrs_minecraft_protocol_game::clientbound::{
    ClientboundBlockUpdate, ClientboundSystemChatPacket,
};
use mcrs_minecraft_protocol_game::{Hand, SwingAnimationKind};
use mcrs_minecraft_registries::CurrentRegistries;
use mcrs_minecraft_resource::{Id, ResourceKeyBuf};
use mcrs_minecraft_text::{RawText, Text};
use mcrs_minecraft_vanilla_block_place::{Spot, replaceable};
use mcrs_minecraft_volume::{BlockVolume, Volume};
use mcrs_voxel::{BlockPos, BlockStateId};
use rustc_hash::FxHashMap;

const OVERWORLD_KEY: &str = "minecraft:overworld";
const DAYLIGHT_SKY_LEVEL: f32 = 15.0;

#[derive(Message, Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub struct BedExplosionRequest {
    pub dimension: Entity,
    pub center: DVec3,
    pub power: f32,
    pub fire: bool,
}

const _: () = assert!(
    align_of::<BedExplosionRequest>() == 8 && size_of::<BedExplosionRequest>() == 40,
    "BedExplosionRequest must stay 8-byte aligned and 40 bytes",
);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct BedBlockEntry {
    pub block: Id<Block>,
    pub is_straw: bool,
    pub state: BedState,
}

const _: () = assert!(
    align_of::<BedBlockEntry>() == 2 && size_of::<BedBlockEntry>() == 6,
    "BedBlockEntry must stay 2-byte aligned and 6 bytes",
);

#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct BedBlockTable(FxHashMap<BlockStateId, BedBlockEntry>);

const _: () = assert!(
    align_of::<BedBlockTable>() == align_of::<FxHashMap<BlockStateId, BedBlockEntry>>(),
    "BedBlockTable alignment must match its inner map",
);

impl BedBlockTable {
    #[must_use]
    pub fn get(&self, state: BlockStateId) -> Option<BedBlockEntry> {
        self.0.get(&state).copied()
    }
}

#[derive(Clone, Debug)]
struct DimensionBedConfig {
    dimension_key: String,
    bed_rule: BedRuleSpec,
    straw_bed_rule: BedRuleSpec,
}

#[derive(Resource, Clone, Debug, Default)]
pub struct BakedDimensionBeds(FxHashMap<Entity, DimensionBedConfig>);

const _: () = assert!(
    align_of::<BakedDimensionBeds>() == align_of::<FxHashMap<Entity, DimensionBedConfig>>(),
    "BakedDimensionBeds alignment must match its inner map",
);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VanillaBedsPlugin;

const _: () = assert!(
    align_of::<VanillaBedsPlugin>() == 1 && size_of::<VanillaBedsPlugin>() == 0,
    "VanillaBedsPlugin must stay a zero-sized marker type",
);

impl Plugin for VanillaBedsPlugin {
    fn build(&self, app: &mut App) {
        let _ = app.try_register_required_components::<Player, PlayerFileState>();
        let ready = resource_exists::<CurrentRegistries>.and_then(resource_exists::<Blocks>);
        let changed = resource_exists_and_changed::<CurrentRegistries>
            .or_else(resource_exists_and_changed::<Blocks>);
        app.init_resource::<BedBlockTable>()
            .init_resource::<BakedDimensionBeds>()
            .add_message::<UseItemOnRequest>()
            .add_message::<BlockSetRequest>()
            .add_message::<BlockPlaced>()
            .add_message::<OutboundPlayerPacket>()
            .add_message::<SwingPlayer>()
            .add_message::<BedExplosionRequest>()
            .add_systems(
                FixedUpdate,
                (
                    bake_tables
                        .run_if(ready.and_then(changed))
                        .in_set(TickSet::DomainInput),
                    bake_dimensions
                        .run_if(dimensions_need_bake)
                        .in_set(TickSet::DomainInput),
                    use_bed_item
                        .run_if(on_message::<UseItemOnRequest>)
                        .in_set(TickSet::ClientInput)
                        .after(BlockBreakSet),
                    partner_break
                        .run_if(on_message::<BlockPlaced>)
                        .in_set(TickSet::WorldWrite)
                        .after(BlockSimulationSet),
                ),
            );
    }
}

type DimensionBakeFilter = (
    With<Dimension>,
    Or<(
        Added<Dimension>,
        Changed<DimensionTypeId>,
        Changed<ResourceKeyBuf<DimensionKey>>,
    )>,
);

#[cfg_attr(mcrs_lint, mcrs::event_condition)]
fn dimensions_need_bake(
    registries: Option<Res<CurrentRegistries>>,
    changed: Query<(), DimensionBakeFilter>,
) -> bool {
    !changed.is_empty() || registries.is_some_and(|registry| registry.is_changed())
}

fn bed_at(
    view: &DimensionView<'_>,
    table: &BedBlockTable,
    block_pos: BlockPos,
) -> Option<BedBlockEntry> {
    view.block(block_pos)
        .ok()
        .and_then(|state| table.get(state))
}

fn rule_for_entry(config: Option<&DimensionBedConfig>, bed: BedBlockEntry) -> BedRuleSpec {
    match (config, bed.is_straw) {
        (Some(item), true) => item.straw_bed_rule,
        (Some(item), false) => item.bed_rule,
        (None, true) => OVERWORLD_STRAW_BED_RULE,
        (None, false) => OVERWORLD_BED_RULE,
    }
}

fn bake_tables(
    registries: Res<CurrentRegistries>,
    blocks: Res<Blocks>,
    mut table: ResMut<BedBlockTable>,
) {
    let mut entries = FxHashMap::default();
    let tags = registries.tags::<Block>();
    let beds_tag = tags.as_ref().and_then(|map| map.id(block_tag_keys::BEDS));
    let facings = [
        ("north", BedFacing::North),
        ("south", BedFacing::South),
        ("west", BedFacing::West),
        ("east", BedFacing::East),
    ];
    for entry in blocks.blocks() {
        let Some(block) = blocks.block_index(entry.base_state_id) else {
            continue;
        };
        let is_straw = entry.identifier.as_str() == Block::StrawBed.as_static_str();
        let in_tag = tags
            .as_ref()
            .zip(beds_tag)
            .is_some_and(|(map, tag)| map.contains(tag, block));
        if !is_straw && !in_tag {
            continue;
        }
        for (facing_name, facing) in facings {
            for (part_name, part) in [("foot", BedPart::Foot), ("head", BedPart::Head)] {
                for (occupied_name, occupied) in [("false", false), ("true", true)] {
                    if let Some(id) = bed_state(entry, part_name, facing_name, occupied_name) {
                        let state = BedState {
                            facing,
                            part,
                            occupied,
                        };
                        let bed = BedBlockEntry {
                            block,
                            is_straw,
                            state,
                        };
                        entries.insert(id, bed);
                    }
                }
            }
        }
    }
    table.set_if_neq(BedBlockTable(entries));
}

fn bake_dimensions(
    registries: Option<Res<CurrentRegistries>>,
    dimensions: Query<
        (
            Entity,
            Option<&ResourceKeyBuf<DimensionKey>>,
            Option<&DimensionTypeId>,
        ),
        With<Dimension>,
    >,
    mut baked: ResMut<BakedDimensionBeds>,
) {
    baked.0.clear();
    let registries = registries.as_deref();
    let context = EnvironmentContext { ticks: &[] };
    let convert = |condition| match condition {
        EnvBedCondition::Always => BedRuleCondition::Always,
        EnvBedCondition::WhenDark => BedRuleCondition::WhenDark,
        EnvBedCondition::Never => BedRuleCondition::Never,
    };
    for (entity, key, type_id) in dimensions.iter() {
        let dimension_key = key.map_or_else(
            || OVERWORLD_KEY.to_owned(),
            |item| item.location().to_string(),
        );
        let exploding = key.is_some_and(|item| {
            *item == dimension_keys::THE_NETHER || *item == dimension_keys::THE_END
        });
        let attributes = registries
            .zip(key)
            .zip(type_id)
            .and_then(|((registry, key_id), id)| {
                DimensionAttributes::of_dimension(registry, key_id, id.0).ok()
            });
        let read = |attribute, fallback: BedRuleSpec| {
            let fallback = if exploding {
                EXPLODING_BED_RULE
            } else {
                fallback
            };
            let value = attributes
                .as_ref()
                .and_then(|map| map.value(attribute, &context));
            let Some(AttributeValue::BedRule(rule)) = value else {
                return fallback;
            };
            BedRuleSpec {
                can_sleep: convert(rule.can_sleep),
                can_set_spawn: convert(rule.can_set_spawn),
                destroy_on_use: rule.destroy_on_use,
                destroy_on_leave: rule.destroy_on_leave,
            }
        };
        let bed_rule = read(EnvironmentAttribute::GameplayBedRule, OVERWORLD_BED_RULE);
        let straw_bed_rule = read(
            EnvironmentAttribute::GameplayStrawBedRule,
            OVERWORLD_STRAW_BED_RULE,
        );
        let config = DimensionBedConfig {
            dimension_key,
            bed_rule,
            straw_bed_rule,
        };
        baked.0.insert(entity, config);
    }
}

#[derive(SystemParam)]
struct BedEnvironment<'w, 's> {
    world: WorldView<'w, 's>,
    table: Res<'w, BedBlockTable>,
    beds: Res<'w, BakedDimensionBeds>,
    definitions: Option<Res<'w, Blocks>>,
    items: Option<Res<'w, Items>>,
}

#[derive(SystemParam)]
struct BedOutput<'w, 's> {
    packets: MessageWriter<'w, OutboundPlayerPacket>,
    blocks: MessageWriter<'w, BlockSetRequest>,
    swings: MessageWriter<'w, SwingPlayer>,
    explosions: MessageWriter<'w, BedExplosionRequest>,
    commands: Commands<'w, 's>,
}

impl BedOutput<'_, '_> {
    fn write_block(
        &mut self,
        dimension: Entity,
        session: PlayerSession,
        block_pos: BlockPos,
        state: BlockStateId,
    ) {
        let mut request = remove_block(dimension, block_pos);
        request.new_state = state;
        self.blocks.write(request);
        let packet = ClientboundBlockUpdate {
            block_pos,
            block_state_id: BlockStateNumber(state.number()),
        };
        self.packets
            .write(to(session, PacketPayload::BlockUpdate(packet)));
    }
}

#[derive(QueryData)]
#[query_data(mutable)]
struct BedPlayer {
    owner: &'static Owner,
    mode: &'static PlayerGameMode,
    body: PlayerBody,
    dimension: &'static InDimension,
    transform: &'static Transform,
    file_state: &'static mut PlayerFileState,
    shift: &'static ShiftKey,
    slots: Option<&'static Slots>,
    selected: Option<&'static Selected>,
}

fn use_bed_item(
    mut requests: MessageReader<UseItemOnRequest>,
    mut actors: Query<BedPlayer, With<Player>>,
    stacks: Query<&Kind, With<Count>>,
    environment: BedEnvironment,
    mut output: BedOutput,
    mut handled: Local<Vec<Entity>>,
    mut exploded: Local<Vec<BlockPos>>,
) {
    handled.clear();
    exploded.clear();
    for request in requests.read() {
        let cursor = request.cursor.to_array();
        let valid =
            request.sequence.0 >= 0 && cursor.iter().all(|value| (-0.5..=1.5).contains(value));
        if !valid || handled.contains(&request.entity) {
            continue;
        }
        let Ok(actor) = actors.get(request.entity) else {
            continue;
        };
        let Some(view) = environment.world.get(actor.dimension.0) else {
            continue;
        };
        let stack = held_slot(request.hand, actor.selected, actor.slots);
        let held = stack.is_some_and(|item| stacks.contains(item));
        let bed = if actor.shift.0 && held {
            None
        } else {
            bed_at(&view, &environment.table, request.block_pos)
        };
        if let Some(bed_entry) = bed {
            handled.push(request.entity);
            interact_bed(
                request,
                bed_entry,
                &mut actors,
                &environment,
                &mut output,
                &mut exploded,
            );
        } else if place_bed(request, &actors, &stacks, &environment, &mut output).is_some() {
            handled.push(request.entity);
        }
    }
}

fn interact_bed(
    request: &UseItemOnRequest,
    bed: BedBlockEntry,
    actors: &mut Query<BedPlayer, With<Player>>,
    environment: &BedEnvironment<'_, '_>,
    output: &mut BedOutput<'_, '_>,
    exploded: &mut Vec<BlockPos>,
) {
    let Ok(mut actor) = actors.get_mut(request.entity) else {
        return;
    };
    let (dimension, session) = (actor.dimension.0, actor.owner.0);
    let pair = BedPair::from_block(request.block_pos, bed.state);
    let config = environment.beds.0.get(&dimension);
    let rule = rule_for_entry(config, bed);
    let mut input = SleepCheckInput::new(rule, DAYLIGHT_SKY_LEVEL);
    let point = actor.transform.translation;
    let world_point = WorldPoint {
        x: point.x,
        y: point.y,
        z: point.z,
    };
    input.within_reach = bed_in_reach(world_point, pair);
    let outcome = evaluate_bed_use(&input);
    let swing = SwingPlayer::new(request.entity, request.hand, SwingAnimationKind::Whack, 6);
    if matches!(outcome, BedUseOutcome::Explode) {
        if !exploded.contains(&pair.head) {
            exploded.push(pair.head);
            output.write_block(dimension, session, pair.head, BlockStateId::AIR);
            output.write_block(dimension, session, pair.foot, BlockStateId::AIR);
            let origin = explosion_origin(pair.head);
            let center = DVec3::new(origin.x, origin.y, origin.z);
            output.explosions.write(BedExplosionRequest {
                dimension,
                center,
                power: BED_EXPLOSION_SPEC.power,
                fire: BED_EXPLOSION_SPEC.creates_fire,
            });
            output.swings.write(swing);
        }
        return;
    }
    let (sets_spawn, should_swing) = match outcome {
        BedUseOutcome::Rejected { reason, sets_spawn } => {
            (sets_spawn, !matches!(reason, SleepRejection::TooFarAway))
        }
        BedUseOutcome::SleepStarted => (
            can_condition_pass(rule.can_set_spawn, DAYLIGHT_SKY_LEVEL),
            true,
        ),
        _ => (false, true),
    };
    if sets_spawn {
        let dimension = config
            .map_or(OVERWORLD_KEY, |item| item.dimension_key.as_str())
            .to_owned();
        let target = Respawn {
            dimension,
            pos: Ints([pair.head.x, pair.head.y, pair.head.z]),
            yaw: 0.0,
            pitch: 0.0,
            forced: false,
        };
        if actor.file_state.respawn.as_ref() != Some(&target) {
            actor.file_state.respawn = Some(target);
            let text: Text = Text::translate("block.minecraft.set_spawn", Vec::new());
            if let Ok(content) = RawText::from_text(&text) {
                let packet = ClientboundSystemChatPacket {
                    content,
                    overlay: false,
                };
                output
                    .packets
                    .write(to(session, PacketPayload::SystemChat(packet)));
            }
        }
    }
    if should_swing {
        output.swings.write(swing);
    }
}

fn place_bed(
    request: &UseItemOnRequest,
    actors: &Query<BedPlayer, With<Player>>,
    stacks: &Query<&Kind, With<Count>>,
    environment: &BedEnvironment<'_, '_>,
    output: &mut BedOutput<'_, '_>,
) -> Option<()> {
    let definitions = environment.definitions.as_deref()?;
    let items = environment.items.as_deref()?;
    let actor = actors.get(request.entity).ok()?;
    if matches!(actor.mode.0, GameMode::Spectator | GameMode::Adventure) {
        return None;
    }
    let view = environment.world.get(actor.dimension.0)?;
    let clicked = view.block(request.block_pos).ok()?;
    let stack = held_slot(request.hand, actor.selected, actor.slots)?;
    let kind = stacks.get(stack).ok()?;
    let block_state = items.get(Id::checked(kind.0, items.len())?)?.block_placer?;
    let block_id = definitions.block_index(block_state)?;
    let entry = definitions.entry(block_id)?;
    environment.table.get(entry.default_state_id)?;
    let eye = eye_position(actor.transform.translation, actor.body.dimensions());
    if !reach(actor.body.attributes).is_some_and(|limit| in_reach(eye, request.block_pos, limit)) {
        return None;
    }
    let can_place_at = |block_pos| {
        view.block(block_pos)
            .ok()
            .is_some_and(|state| replaceable(definitions, state, block_id, Spot::Adjacent))
    };
    let adjacent = request.block_pos + request.face.normal();
    let foot = if replaceable(definitions, clicked, block_id, Spot::Clicked) {
        request.block_pos
    } else if can_place_at(adjacent) {
        adjacent
    } else {
        return None;
    };
    let (facing, facing_name) = match Direction::from_y_rot(actor.transform.rotation.yaw()) {
        Direction::North => (BedFacing::North, "north"),
        Direction::South => (BedFacing::South, "south"),
        Direction::West => (BedFacing::West, "west"),
        _ => (BedFacing::East, "east"),
    };
    let pair = BedPair::from_placement(foot, facing);
    let range = view.bounds().min.y..=view.bounds().max.y;
    if !range.contains(&pair.foot.y) || !range.contains(&pair.head.y) || !can_place_at(pair.head) {
        return None;
    }
    let new_foot = bed_state(entry, "foot", facing_name, "false")?;
    let new_head = bed_state(entry, "head", facing_name, "false")?;
    if overlaps_any_player(actor.dimension.0, &pair, actors) {
        return None;
    }
    if actor.mode.0 != GameMode::Creative {
        output.commands.entity(stack).consume(1);
    }
    output.write_block(actor.dimension.0, actor.owner.0, pair.foot, new_foot);
    output.write_block(actor.dimension.0, actor.owner.0, pair.head, new_head);
    let swing = SwingPlayer::new(request.entity, request.hand, SwingAnimationKind::Whack, 6);
    output.swings.write(swing);
    Some(())
}

fn partner_break(
    mut placed: MessageReader<BlockPlaced>,
    dimensions: Query<(Entity, &SectionIndex), With<Dimension>>,
    world: WorldView,
    table: Res<BedBlockTable>,
    mut requests: MessageWriter<BlockSetRequest>,
) {
    for event in placed.read() {
        if let Some(old_bed) = table.get(event.old_state) {
            let unchanged = table.get(event.new_state).is_some_and(|next| {
                next.block == old_bed.block
                    && next.state.facing == old_bed.state.facing
                    && next.state.part == old_bed.state.part
            });
            let partner = old_bed.state.partner_pos(event.block_pos);
            if !unchanged
                && let Some((dimension, _)) = dimensions
                    .iter()
                    .find(|(_, index)| index.get(event.chunk_pos) == Some(event.chunk))
                && let Some(view) = world.get(dimension)
                && let Some(partner_bed) = bed_at(&view, &table, partner)
                && partner_bed.block == old_bed.block
                && old_bed.state.is_expected_partner(partner_bed.state)
            {
                requests.write(remove_block(dimension, partner));
            }
        }
    }
}

fn bed_state(entry: &BlockEntry, part: &str, facing: &str, occupied: &str) -> Option<BlockStateId> {
    let base = entry.with_text(entry.default_state_id, "part", part)?;
    let faced = entry.with_text(base, "facing", facing)?;
    entry.with_text(faced, "occupied", occupied)
}

fn held_slot(hand: Hand, selected: Option<&Selected>, slots: Option<&Slots>) -> Option<Entity> {
    let index = match hand {
        Hand::Main => usize::from(selected?.0),
        Hand::Off => usize::from(OFF_HAND),
    };
    slots?.get(index)
}

fn bed_aabb(block_pos: BlockPos) -> Aabb {
    let origin = block_pos.as_dvec3();
    Aabb::new(origin, origin + DVec3::new(1.0, BED_TOP_OFFSET, 1.0))
}

fn overlaps_any_player(
    dimension: Entity,
    pair: &BedPair,
    actors: &Query<BedPlayer, With<Player>>,
) -> bool {
    let (foot_box, head_box) = (bed_aabb(pair.foot), bed_aabb(pair.head));
    actors.iter().any(|actor| {
        let dimensions = actor.body.dimensions();
        let body = Aabb::of_body(
            actor.transform.translation,
            dimensions.width,
            dimensions.height,
        );
        actor.dimension.0 == dimension
            && actor.mode.0 != GameMode::Spectator
            && (foot_box.intersects(&body) || head_box.intersects(&body))
    })
}
