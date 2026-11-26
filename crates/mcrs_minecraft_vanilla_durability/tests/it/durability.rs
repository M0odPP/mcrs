use bevy_app::{App, FixedUpdate};
use bevy_ecs::entity::Entity;
use bevy_ecs::message;
use bevy_ecs::schedule::{self, IntoScheduleConfigs};
use mcrs_minecraft_block::definition as block_definition;
use mcrs_minecraft_damage as damage;
use mcrs_minecraft_engine::bus::{OutboundPlayerPacket, PacketPayload::EntityEvent as PacketEvent};
use mcrs_minecraft_engine::player_signal::PlayerSignal;
use mcrs_minecraft_engine_damage as engine_damage;
use mcrs_minecraft_engine_item::{self as engine_item, ItemCommandsExt, StackCommandsExt};
use mcrs_minecraft_engine_world as engine_world;
use mcrs_minecraft_game_mode::{GameMode, PlayerGameMode};
use mcrs_minecraft_inventory_player::ArmourSlot;
use mcrs_minecraft_item_stack as item_stack;
use mcrs_minecraft_vanilla_durability::DurabilitySlot::{self, Armour};

fn test_app() -> App {
    use engine_world::tick::TickSet::*;
    use item_stack::*;
    let gear = vec![
        Builtin::MaxDamage(MaxDamage(4)),
        Builtin::Damage(Damage(0)),
        Builtin::Tool(Tool::new(&[])),
        Builtin::Weapon(Weapon::new()),
        Builtin::Equippable(Equippable::new(EquippableSlot::Head)),
    ];
    let mut app = App::new();
    let air = include_bytes!("../../../../assets/mcrs/block_definition/air.json").to_vec();
    let stone = include_bytes!("../../../../assets/mcrs/block_definition/stone.json").to_vec();
    let parsed = block_definition::build_block_definitions(
        &["minecraft:air", "minecraft:stone"],
        [("air.json".into(), air), ("stone.json".into(), stone)],
    );
    assert!(parsed.is_ok(), "stone block definition must parse");
    if let Ok((defs, _)) = parsed {
        app.insert_resource(block_definition::Blocks(std::sync::Arc::new(defs)));
    }
    let table = mcrs_minecraft_item_table::ItemTable::leak(vec![vec![], gear]);
    app.insert_resource(table);
    app.insert_resource(damage::DamageTypeRules::new([damage::DamageTags::empty()]));
    let ticks = (
        DomainInput,
        ClientInput,
        Simulate,
        WorldWrite,
        Project,
        Output,
    )
        .chain();
    app.configure_sets(FixedUpdate, (ticks, engine_item::ItemSet.in_set(Simulate)));
    app.add_plugins((
        engine_damage::DamagePlugin,
        engine_item::ItemPersistencePlugin,
        engine_item::ItemPlugin::default(),
        mcrs_minecraft_vanilla_durability::DurabilityPlugin,
    ));
    app.finish();
    app.cleanup();
    app
}

fn spawn_player(app: &mut App, session_id: u64, mode: GameMode) -> Entity {
    use engine_world::session::{Owner, PlayerSession};
    let owner = Owner(PlayerSession(session_id));
    let inventory = (engine_item::Selected(0), engine_item::Slots::new(41));
    let uuid = mcrs_minecraft_entity_key::EntityUuid::from_u128(u128::from(session_id));
    app.world_mut()
        .spawn((owner, PlayerGameMode(mode), inventory, uuid))
        .id()
}

fn put(app: &mut App, holder: Entity, slot: u16, stack: item_stack::ItemStack) -> Entity {
    app.world_mut()
        .commands()
        .spawn_stack(stack)
        .put_at(holder, slot)
        .id()
}

fn extra(app: &mut App, target: Entity, slot: DurabilitySlot, amount: i32) {
    app.world_mut()
        .write_message(mcrs_minecraft_vanilla_durability::DurabilityExtraUse {
            target,
            slot,
            amount,
        });
}

fn damage_of(app: &App, entity: Entity) -> Option<i32> {
    app.world()
        .get::<item_stack::Damage>(entity)
        .map(|damage| damage.0)
}

fn queue_component_events(
    app: &mut App,
    stone: mcrs_voxel::BlockStateId,
    [victim, miner, attacker]: [Entity; 3],
) {
    let world = app.world_mut();
    world.write_message(engine_world::block_drops::BlockBroken {
        at: mcrs_voxel::BlockPos::new(0, 64, 0),
        state: stone,
        left: stone,
        other: None,
        harvests: true,
        breaker: miner,
    });
    world.write_message(engine_damage::MeleeAttackHit {
        attacker,
        target: victim,
    });
    world.write_message(engine_damage::Damaged {
        target: victim,
        source: damage::DamageSource::of(0),
        report: damage::HitReport {
            outcome: damage::HitOutcome::Full,
            raw_amount: 8.0,
            armor: 0.0,
            absorption: 0.0,
            health: 12.0,
            died: false,
        },
    });
}

fn read_statuses<M: message::Message>(app: &App, pick: fn(&M) -> Option<i8>) -> Vec<i8> {
    let msgs = app.world().resource::<message::Messages<M>>();
    msgs.get_cursor().read(msgs).filter_map(pick).collect()
}

#[test]
fn durability_schedules_have_no_ambiguities() {
    let mut app = test_app();
    let schedules = app.world().resource::<schedule::Schedules>();
    let labels: Vec<_> = schedules.iter().map(|(_, entry)| entry.label()).collect();
    assert!(!labels.is_empty(), "schedules must be registered");
    for label in labels {
        app.world_mut().schedule_scope(label, |world, entry| {
            let settings = schedule::ScheduleBuildSettings {
                ambiguity_detection: schedule::LogLevel::Error,
                ..entry.get_build_settings().clone()
            };
            entry.set_build_settings(settings);
            let res = entry.initialize(world);
            assert!(res.is_ok(), "schedule {label:?} ambiguity");
        });
    }
}

#[test]
fn extra_use_wears_breaks_deduplicates_and_respects_removed_components() {
    use item_stack::*;
    let mut app = test_app();
    let base = ItemStack::new(Kind(1), Count::new(1).expect("count 1"));
    let [survival, breaker, victim, miner, attacker] =
        [10, 11, 30, 31, 32].map(|id| spawn_player(&mut app, id, GameMode::Survival));
    let creative = spawn_player(&mut app, 20, GameMode::Creative);
    let head = ArmourSlot::Head.index();
    let near_break = base.clone().set(Damage(3));
    let sword = put(&mut app, survival, 0, base.clone());
    let helmet = put(&mut app, survival, head, near_break.clone());
    let offhand = put(&mut app, survival, 40, base.clone().set(Damage(2)));
    let doubled = put(&mut app, breaker, 0, near_break);
    let unbreakable = put(&mut app, breaker, 40, base.clone().set(Unbreakable));
    let creative_tool = put(&mut app, creative, 0, base.clone());
    let no_damage = put(&mut app, victim, 0, base.clone().unset::<Damage>());
    let no_max = put(&mut app, victim, 40, base.clone().unset::<MaxDamage>());
    let no_equippable = put(&mut app, victim, head, base.clone().unset::<Equippable>());
    let no_tool = put(&mut app, miner, 0, base.clone().unset::<Tool>());
    let no_weapon = put(&mut app, attacker, 0, base.unset::<Weapon>());
    app.world_mut().flush();
    let blocks = app.world().resource::<block_definition::Blocks>();
    let stone = blocks.default_state("minecraft:stone").expect("stone");
    use DurabilitySlot::{MainHand, OffHand};
    extra(&mut app, survival, MainHand, 2);
    extra(&mut app, survival, Armour(ArmourSlot::Head), 1);
    extra(&mut app, survival, OffHand, 2);
    extra(&mut app, breaker, MainHand, 1);
    extra(&mut app, breaker, MainHand, 1);
    extra(&mut app, breaker, OffHand, 2);
    extra(&mut app, creative, MainHand, 2);
    extra(&mut app, victim, MainHand, 2);
    extra(&mut app, victim, OffHand, 2);
    queue_component_events(&mut app, stone, [victim, miner, attacker]);
    app.world_mut().run_schedule(FixedUpdate);
    assert!(app.world().get_entity(helmet).is_err(), "helmet break");
    assert!(app.world().get_entity(offhand).is_err(), "offhand break");
    assert!(app.world().get_entity(doubled).is_err(), "doubled break");
    assert_eq!(damage_of(&app, sword), Some(2), "sword wear");
    assert_eq!(damage_of(&app, unbreakable), Some(0), "unbreakable skipped");
    assert_eq!(damage_of(&app, creative_tool), Some(0), "creative skipped");
    assert_eq!(damage_of(&app, no_damage), None, "unset Damage");
    assert_eq!(damage_of(&app, no_max), Some(0), "unset MaxDamage");
    assert_eq!(damage_of(&app, no_equippable), Some(0), "unset Equippable");
    assert_eq!(damage_of(&app, no_tool), Some(0), "unset Tool");
    assert_eq!(damage_of(&app, no_weapon), Some(0), "unset Weapon");
    let packets = read_statuses::<OutboundPlayerPacket>(&app, |pkt| match &pkt.data {
        PacketEvent(ev) => Some(ev.entity_status),
        _ => None,
    });
    let signals = read_statuses::<PlayerSignal>(&app, |entry| match entry.signal {
        mcrs_minecraft_replication::MobSignal::EntityEvent { status } => Some(status),
        _ => None,
    });
    assert_eq!(packets, [52, 48, 47], "packet statuses");
    assert_eq!(signals, [52, 48, 47], "signal statuses");
}
