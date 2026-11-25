use bevy_ecs::entity::Entity;
use bevy_math::DVec3;
use mcrs_minecraft_block::definition::Blocks;
use mcrs_minecraft_command_engine::CommandPlugin;
use mcrs_minecraft_engine::bus;
use mcrs_minecraft_engine::client_on_ground::ClientOnGround;
use mcrs_minecraft_engine_item::{ItemCommandsExt, Slots, StackCommandsExt};
use mcrs_minecraft_engine_registries as engine_registries;
use mcrs_minecraft_engine_world::entity::physics::Transform;
use mcrs_minecraft_engine_world::world::dimension::InDimension;
use mcrs_minecraft_item_key::item_keys::Item;
use mcrs_minecraft_item_stack::{self as item_stack, ItemStack};
use mcrs_minecraft_protocol::VarInt;
use mcrs_minecraft_protocol::packets::table::Decoded;
use mcrs_minecraft_protocol::uuid::Uuid;
use mcrs_minecraft_protocol_game::player::PlayerAction;
use mcrs_minecraft_protocol_game::serverbound;
use mcrs_minecraft_protocol_game::table::game_clientbound::{self, ClientboundGamePacket as Game};
use mcrs_minecraft_registries::CurrentRegistries;
use mcrs_minecraft_server as server_plugins;

use crate::host_app;
use crate::mob_replication::mob_entities;

const ALICE: Uuid = Uuid::from_u128(1);
const DIG_POS: mcrs_voxel::BlockPos = mcrs_voxel::BlockPos::new(9, 65, 8);
const GOLDEN_ARMOUR: [(u16, Item, u16); 4] = [
    (39, Item::GoldenHelmet, 5),
    (38, Item::GoldenChestplate, 6),
    (37, Item::GoldenLeggings, 7),
    (36, Item::GoldenBoots, 8),
];

#[derive(Debug, PartialEq, Eq)]
enum Trace {
    Status(i8),
    Block,
    Health,
    Data,
    Slot(u16, bool),
    Damage,
}

struct Arena {
    server: host_app::PlayServer,
    alice: Entity,
    player: Entity,
    bob: Entity,
}

impl Arena {
    fn start() -> Self {
        use mcrs_minecraft_engine::column_version::ColumnVersion;
        use mcrs_voxel::ColumnPos;
        let mut app = host_app::make_host_app();
        let table = engine_registries::test_item_table();
        let installed = engine_registries::install_item_table(app.world_mut(), table);
        assert!(installed.is_ok(), "item table");
        app.add_plugins((
            mcrs_minecraft_engine::coordinator::birth_clock::BirthClockPlugin,
            CommandPlugin::<mcrs_minecraft_command_vanilla::VanillaCommands>::default(),
            mcrs_minecraft_command_vanilla::VanillaCommandsPlugin,
            server_plugins::VanillaEntityPhysicsPlugin,
            server_plugins::VanillaEntityAttributesPlugin,
            server_plugins::VanillaItemEntityPlugin,
            server_plugins::VanillaItemPlugin,
            server_plugins::VanillaBlocksPlugin,
            server_plugins::VanillaPlayerAttackPlugin,
            server_plugins::VanillaDurabilityPlugin,
        ));
        app.insert_resource(bevy_time::TimeUpdateStrategy::FixedTimesteps(1));
        let mut server = host_app::PlayServer::start(app, &[(ALICE, 2)], 0);
        crate::commands::load_stone_column(&mut server);
        let (alice, _) = server.join(ALICE, "Alice");
        let (bob, _) = server.join(Uuid::from_u128(2), "Bob");
        let batch = serverbound::ServerboundChunkBatchReceived {
            desired_chunks_per_tick: 64.0,
        };
        for client in [alice, bob] {
            server.queue(client, &serverbound::ServerboundPlayerLoaded);
            server.queue(client, &batch);
        }
        let col = ColumnPos::new(0, 0);
        server.world().write_message(ColumnVersion {
            column: col,
            packet: bytes::Bytes::new(),
        });
        let _ = server.ticks_all(3);
        let (player, bob_player) = (server.player(alice), server.player(bob));
        let world = server.world();
        world.get_mut::<ClientOnGround>(player).unwrap().0 = true;
        world.get_mut::<Transform>(bob_player).unwrap().translation = DVec3::new(8.5, 64.0, 10.5);
        let _ = server.ticks_all(3);
        let mut arena = Self {
            server,
            alice,
            player,
            bob,
        };
        let _ = arena.command("gamemode survival Alice");
        arena
    }

    fn command(&mut self, line: &str) -> Vec<bus::OutboundPlayerPacket> {
        let cmd = serverbound::ServerboundChatCommand {
            command: mcrs_minecraft_protocol::Bounded(line),
        };
        self.server.queue(self.alice, &cmd);
        self.server.ticks_all(2)
    }

    fn hurt(&mut self, spec: &str) -> Vec<bus::OutboundPlayerPacket> {
        use mcrs_minecraft_damage::Vitals;
        self.server
            .world()
            .get_mut::<Vitals>(self.player)
            .unwrap()
            .health = 20.0;
        let _ = self.server.ticks_all(10);
        self.command(&format!("damage Alice {spec}"))
    }

    fn slot_entity(&mut self, slot: u16) -> Option<Entity> {
        self.server
            .world()
            .get::<Slots>(self.player)?
            .get(usize::from(slot))
    }

    fn equip(&mut self, slot: u16, stack: ItemStack) {
        if let Some(old) = self.slot_entity(slot) {
            self.server.world().despawn(old);
        }
        let (player, world) = (self.player, self.server.world());
        world.commands().spawn_stack(stack).put_at(player, slot);
        let _ = self.server.ticks_all(1);
    }

    fn equip_armour(&mut self, near_break: bool) {
        for (slot, item, _) in GOLDEN_ARMOUR {
            self.equip(slot, stack_of(item));
            if near_break && let Some(stack) = self.slot_entity(slot) {
                let world = self.server.world();
                let max = world.get::<item_stack::MaxDamage>(stack).unwrap().0;
                world.get_mut::<item_stack::Damage>(stack).unwrap().0 = max - 1;
            }
        }
    }

    fn damage_of(&mut self, slot: u16) -> Option<(Item, i32)> {
        let stack = self.slot_entity(slot)?;
        let world = self.server.world();
        let item = Item::from_protocol_id(world.get::<item_stack::Kind>(stack)?.0)?;
        let dmg = world.get::<item_stack::Damage>(stack).map_or(0, |d| d.0);
        Some((item, dmg))
    }

    fn assert_damage(&mut self, slot: u16, item: Item, damage: i32, reason: &str) {
        assert_eq!(self.damage_of(slot), Some((item, damage)), "{reason}");
    }

    fn assert_no_wear(&mut self, item: Item, packets: &[bus::OutboundPlayerPacket], reason: &str) {
        self.assert_damage(0, item, 0, reason);
        let events = self.events(self.alice, packets);
        let worn = events
            .iter()
            .any(|ev| matches!(ev, Trace::Slot(..) | Trace::Status(47..=52)));
        assert!(!worn, "{reason}: {events:?}");
    }

    fn break_block(&mut self, block: &str, dig_ticks: usize) -> Vec<bus::OutboundPlayerPacket> {
        use mcrs_minecraft_engine_world::block_update::BlockSetRequest;
        let world = self.server.world();
        let new_state = world.resource::<Blocks>().default_state(block).unwrap();
        let dimension = world.get::<InDimension>(self.player).unwrap().0;
        let flags = mcrs_minecraft_simulation::BlockUpdateFlags::all();
        world.write_message(BlockSetRequest {
            dimension,
            pos: DIG_POS,
            new_state,
            flags,
            recursion_left: 1,
        });
        let _ = self.server.ticks_all(2);
        let dig = |action, sequence| serverbound::ServerboundPlayerAction {
            action,
            pos: DIG_POS,
            direction: mcrs_minecraft_core::Direction::West,
            sequence: VarInt(sequence),
        };
        self.server
            .queue(self.alice, &dig(PlayerAction::StartDestroyBlock, 1));
        let mut packets = self.server.ticks_all(dig_ticks);
        self.server
            .queue(self.alice, &dig(PlayerAction::StopDestroyBlock, 2));
        packets.extend(self.server.ticks_all(2));
        packets
    }

    fn attack_entity(&mut self, entity_id: VarInt) -> Vec<bus::OutboundPlayerPacket> {
        self.server
            .queue(self.alice, &serverbound::ServerboundAttack { entity_id });
        self.server.ticks_all(12)
    }

    fn events(&self, client: Entity, packets: &[bus::OutboundPlayerPacket]) -> Vec<Trace> {
        use Trace::{Block, Data, Health, Slot, Status};
        use bus::PacketPayload as P;
        let session = self.server.session_of(client).unwrap();
        let mut events = Vec::new();
        for packet in host_app::sent_to(session, packets.to_vec()) {
            match &packet {
                P::EntityEvent(ev) | P::OpLevelEntityEvent(ev) => {
                    events.push(Status(ev.entity_status))
                }
                P::BlockUpdate(_) => events.push(Block),
                P::SetHealth(_) => events.push(Health),
                P::OwnEntityState(_) => events.push(Data),
                P::ContainerSetSlot(sl) => events.push(Slot(sl.slot as u16, !sl.item.is_empty())),
                P::Replication(_) => push_replicated(&packet, &mut events),
                _ => {}
            }
        }
        events
    }
}

fn push_replicated(packet: &bus::PacketPayload, events: &mut Vec<Trace>) {
    for (id, bytes) in host_app::replicated(std::slice::from_ref(packet)) {
        if let Ok(Decoded::Packet(pkt)) = game_clientbound::decode(id, &mut bytes.as_slice()) {
            match pkt {
                Game::ClientboundEntityEvent(ev) => events.push(Trace::Status(ev.entity_status)),
                Game::ClientboundDamageEvent(_) => events.push(Trace::Damage),
                Game::ClientboundSetEntityData(_) => events.push(Trace::Data),
                _ => {}
            }
        }
    }
}

fn stack_of(item: Item) -> ItemStack {
    ItemStack::new(
        item_stack::Kind(item.id().number()),
        item_stack::Count::new(1).expect("count 1"),
    )
}

fn assert_order(events: &[Trace], ordered: &[Trace]) {
    let pos = |t: &Trace| events.iter().position(|e| e == t);
    for pair in ordered.windows(2) {
        let (first, second) = (pos(&pair[0]), pos(&pair[1]));
        assert!(first.is_some() && first < second, "{pair:?}");
    }
}

fn assert_slot(events: &[Trace], slot: u16, occupied: bool) {
    assert!(events.contains(&Trace::Slot(slot, occupied)), "slot {slot}");
}

#[test]
fn block_break_and_melee_wear_break_and_skip_unbreakable_and_creative() {
    let mut arena = Arena::start();
    let pickaxe = Item::GoldenPickaxe;
    arena.equip(0, stack_of(pickaxe).set(item_stack::Damage(30)));
    let first = arena.break_block("minecraft:stone", 110);
    arena.assert_damage(0, pickaxe, 31, "stone wear");
    assert_slot(&arena.events(arena.alice, &first), 36, true);
    let second = arena.break_block("minecraft:stone", 110);
    assert_eq!(arena.damage_of(0), None, "pickaxe breaks");
    let alice = arena.events(arena.alice, &second);
    let bob = arena.events(arena.bob, &second);
    assert_order(&alice, &[Trace::Status(47), Trace::Block]);
    assert_order(&alice, &[Trace::Status(47), Trace::Slot(36, false)]);
    assert_order(&bob, &[Trace::Status(47), Trace::Block]);
    arena.equip(0, stack_of(pickaxe));
    let slime = arena.break_block("minecraft:slime_block", 1);
    arena.assert_no_wear(pickaxe, &slime, "slime_block");
    let _ = arena.command("gamemode creative Alice");
    let creative = arena.break_block("minecraft:stone", 1);
    arena.assert_no_wear(pickaxe, &creative, "creative break");
    let _ = arena.command("gamemode survival Alice");
    let _ = arena.command("summon minecraft:pig 8.5 64 9.5");
    let pig = mob_entities(&mut arena.server).pop().expect("pig").id;
    let entity_id = VarInt(mcrs_minecraft_replication::wire_id(pig).0);
    arena.equip(0, stack_of(Item::IronSword));
    let hit = arena.attack_entity(entity_id);
    arena.assert_damage(0, Item::IronSword, 1, "melee wear");
    assert_slot(&arena.events(arena.alice, &hit), 36, true);
    arena.equip(0, stack_of(Item::IronSword).set(item_stack::Unbreakable));
    let skipped_sword = arena.attack_entity(entity_id);
    arena.assert_no_wear(Item::IronSword, &skipped_sword, "unbreakable melee");
    arena.equip(0, stack_of(Item::Stick));
    let skipped_stick = arena.attack_entity(entity_id);
    arena.assert_no_wear(Item::Stick, &skipped_stick, "non-weapon melee");
}

#[test]
fn unbreaking_enchantment_reduces_block_and_armour_wear() {
    use mcrs_minecraft_item::{enchantment_keys::UNBREAKING, registry_keys::ENCHANTMENT};
    let mut arena = Arena::start();
    let id = arena
        .server
        .world()
        .resource::<CurrentRegistries>()
        .id_of(ENCHANTMENT.location(), UNBREAKING.location())
        .unwrap();
    let entry = item_stack::EnchantmentLevel {
        enchantment: item_stack::EnchantmentId(id),
        level: std::num::NonZeroU8::new(3).expect("level"),
    };
    let unbreaking = item_stack::Enchantments::new([entry]).expect("enchant");
    arena.equip(0, stack_of(Item::GoldenPickaxe).set(unbreaking.clone()));
    for _ in 0..16 {
        let _ = arena.break_block("minecraft:stone", 110);
    }
    let pickaxe_wear = arena.damage_of(0).unwrap().1;
    assert!(pickaxe_wear > 0 && pickaxe_wear < 16, "tool {pickaxe_wear}");
    arena.equip(38, stack_of(Item::GoldenChestplate).set(unbreaking));
    for _ in 0..16 {
        let _ = arena.hurt("1 minecraft:mob_attack");
    }
    let chest_wear = arena.damage_of(38).unwrap().1;
    assert!(chest_wear > 0 && chest_wear < 16, "armour {chest_wear}");
}

#[test]
fn armour_wear_breaks_sync_slots_and_respect_damage_on_hurt_and_bypass() {
    use Trace::{Damage, Data, Health, Slot, Status};
    let mut arena = Arena::start();
    arena.equip_armour(false);
    let wear_packets = arena.hurt("8 minecraft:mob_attack");
    let wear_events = arena.events(arena.alice, &wear_packets);
    for (slot, item, wire_slot) in GOLDEN_ARMOUR {
        arena.assert_damage(slot, item, 2, "raw 8 -> 2 wear");
        assert_slot(&wear_events, wire_slot, true);
    }
    let early = wear_events.iter().any(|ev| matches!(ev, Status(49..=52)));
    assert!(!early, "early break");
    arena.equip_armour(true);
    let break_packets = arena.hurt("4 minecraft:mob_attack");
    let broken = (36..=39).all(|slot| arena.damage_of(slot).is_none());
    assert!(broken, "armour break");
    let alice = arena.events(arena.alice, &break_packets);
    let bob = arena.events(arena.bob, &break_packets);
    let order = [Status(52), Status(51), Status(50), Status(49), Damage];
    assert_order(&alice, &order);
    assert_order(&bob, &order);
    assert_order(&alice, &[Status(49), Data]);
    assert_order(&alice, &[Status(49), Health]);
    let cleared = (5..=8).all(|slot| alice.contains(&Slot(slot, false)));
    assert!(cleared, "cleared slots");
    arena.equip(39, stack_of(Item::GoldenHelmet));
    arena.equip(38, stack_of(Item::Elytra));
    let _ = arena.hurt("8 minecraft:out_of_world");
    arena.assert_damage(39, Item::GoldenHelmet, 0, "bypass helmet");
    arena.assert_damage(38, Item::Elytra, 0, "bypass elytra");
    let _ = arena.hurt("8 minecraft:mob_attack");
    arena.assert_damage(39, Item::GoldenHelmet, 2, "mob_attack helmet");
    arena.assert_damage(38, Item::Elytra, 0, "damage_on_hurt=false elytra");
}
