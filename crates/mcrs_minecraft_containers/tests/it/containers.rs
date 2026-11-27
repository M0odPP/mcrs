use mcrs_minecraft_containers::{
    BarrelOpenState, ChestHalf, ChestSpan, ChestType, Clearance, ContainerFacing, ContainerKind,
    ContainerLayout, ContainerLidState, ContainerMenuKind, CopperChestVariant, InsertedItem,
    LidFeedback, LidSoundTrigger, LidTransition, ObstructionRule, can_insert_into_container,
    can_insert_item_id, can_open_double, can_open_single, can_pair_halves, classify_block,
    double_chest_slot, obstructing_pos, obstruction_rule, pair_on_place, partner_pos,
};
use mcrs_test_alloc::allocations;
use mcrs_voxel::BlockPos;

fn half(pos: BlockPos, kind: ContainerKind, chest_type: ChestType) -> ChestHalf {
    let facing = ContainerFacing::North;
    ChestHalf {
        pos,
        kind,
        facing,
        chest_type,
    }
}

#[test]
fn classify_containers_and_sounds() {
    use ContainerMenuKind::{Generic3x3, Generic9x3, Generic9x6, Hopper, ShulkerBox};
    let single = |id| classify_block(id, ChestSpan::Single);
    let check_menu = |id, want| assert_eq!(single(id).map(|entry| entry.menu), Some(want));
    check_menu("cyan_shulker_box", ShulkerBox);
    check_menu("ender_chest", Generic9x3);
    check_menu("dispenser", Generic3x3);
    check_menu("dropper", Generic3x3);
    check_menu("hopper", Hopper);

    let hopper = ContainerKind::Hopper;
    let custom = ContainerLayout::new(
        hopper,
        Hopper,
        "container.hopper",
        None,
        LidFeedback::None,
        ObstructionRule::None,
    );
    assert_eq!((custom.slot_count, Hopper.total_menu_slots()), (5, 41));
    let chest = single("minecraft:chest");
    let double = classify_block("chest", ChestSpan::Double);
    let barrel = single("barrel");
    let found = chest.is_some() && double.is_some() && barrel.is_some();
    assert!(found, "present");
    if let (Some(chest), Some(double), Some(barrel)) = (chest, double, barrel) {
        assert_eq!(chest.menu.key(), "minecraft:generic_9x3");
        assert_eq!(chest.default_title_key, "container.chest");
        assert_eq!(chest.lid_feedback, LidFeedback::BlockEvent);
        assert_eq!((double.menu, double.slot_count), (Generic9x6, 54));
        assert_eq!(double.default_title_key, "container.chestDouble");
        let close = barrel.lid_sounds.map(|pair| pair.close);
        assert_eq!(barrel.menu, Generic9x3);
        assert_eq!(barrel.lid_feedback, LidFeedback::OpenProperty);
        assert_eq!(close, Some("minecraft:block.barrel.close"));
    }
    let prefix = "minecraft:block.";
    let open = |id| single(id).and_then(|entry| entry.lid_sounds?.open.strip_prefix(prefix));
    assert_eq!(open("trapped_chest"), Some("chest.open"));
    assert_eq!(open("copper_chest"), Some("copper_chest.open"));
    let weathered = Some("copper_chest_weathered.open");
    let oxidized = Some("copper_chest_oxidized.open");
    assert_eq!(open("weathered_copper_chest"), weathered);
    assert_eq!(open("waxed_oxidized_copper_chest"), oxidized);
    assert_eq!(open("barrel"), Some("barrel.open"));
    assert_eq!(open("dispenser"), None);
    assert_eq!(single("stone"), None);
    assert_eq!(ContainerKind::from_block_entity_id("hopper"), Some(hopper));
}

#[test]
fn double_chest_pairing_and_slots() {
    use ChestType::{Left, Right, Single};
    use CopperChestVariant::{Exposed, Unaffected, WaxedUnaffected};
    let chest = ContainerKind::Chest;
    let north = ContainerFacing::North;
    let (right_pos, left_pos) = (BlockPos::new(0, -60, 2), BlockPos::new(-1, -60, 2));
    let (right, left) = (half(right_pos, chest, Right), half(left_pos, chest, Left));
    let can_pair = can_pair_halves(right, left) && can_pair_halves(left, right);
    assert!(can_pair, "pair");
    assert_eq!(partner_pos(right_pos, north, Right), Some(left_pos));

    let single = half(right_pos, chest, Single);
    let placed = pair_on_place(left_pos, chest, north, single).map(|pair| pair.placed);
    assert_eq!(placed, Some(Left));
    let copper = ContainerKind::CopperChest;
    let copper_half = half(right_pos, copper(Unaffected), Single);
    let reject = |kind, facing, target| {
        assert_eq!(pair_on_place(left_pos, kind, facing, target), None);
    };
    reject(ContainerKind::TrappedChest, north, single);
    reject(chest, ContainerFacing::South, single);
    reject(copper(WaxedUnaffected), north, copper_half);
    reject(copper(Exposed), north, copper_half);

    let slot_at = |index| double_chest_slot(index).map(|entry| (entry.half, entry.local_slot));
    assert_eq!(slot_at(0), Some((Right, 0)));
    assert_eq!(slot_at(26), Some((Right, 26)));
    assert_eq!(slot_at(27), Some((Left, 0)));
    assert_eq!(slot_at(53), Some((Left, 26)));
    assert_eq!(slot_at(54), None);
}

#[test]
fn obstruction_nesting_and_lid_transitions() {
    use ContainerFacing::{North, South, Up};
    use ContainerKind::{Barrel, Chest, EnderChest, ShulkerBox};
    let pos = BlockPos::new(0, -60, 3);
    let block_pos = BlockPos::new;
    let obstruction = |kind, face| obstructing_pos(pos, kind, face);
    assert_eq!(obstruction(Chest, North), Some(block_pos(0, -59, 3)));
    assert_eq!(obstruction(Barrel, Up), None);
    assert_eq!(obstruction(ShulkerBox, South), Some(block_pos(0, -60, 4)));
    let open_ok = |kind| can_open_single(obstruction_rule(kind), Clearance::Blocked);
    let double_ok = can_open_double(Clearance::Clear, Clearance::Blocked);
    let obstruction_ok = open_ok(Barrel) && !open_ok(EnderChest) && !double_ok;
    assert!(obstruction_ok, "obstruction");

    let no_box = !can_insert_into_container(ShulkerBox, InsertedItem::ShulkerBox);
    let no_dyed = !can_insert_item_id(ShulkerBox, "minecraft:white_shulker_box");
    let gold = can_insert_item_id(ShulkerBox, "minecraft:gold_ingot");
    let in_chest = can_insert_item_id(Chest, "minecraft:shulker_box");
    assert!(no_box && no_dyed && gold && in_chest, "nest");

    let mut chest = ContainerLidState::new();
    let event = LidFeedback::BlockEvent;
    let silent = LidSoundTrigger::None;
    let check = |step: LidTransition, sound, param| {
        assert_eq!((step.sound, step.block_event_param), (sound, param));
    };
    check(chest.open(event), LidSoundTrigger::Open, Some(1));
    check(chest.open(event), silent, Some(2));
    check(chest.close(event), silent, Some(1));
    check(chest.close(event), LidSoundTrigger::Close, Some(0));
    check(chest.close(event), silent, None);

    let mut dispenser = ContainerLidState::new();
    check(dispenser.open(LidFeedback::None), silent, None);
    check(dispenser.close(LidFeedback::None), silent, None);

    let mut barrel = ContainerLidState::new();
    let property = LidFeedback::OpenProperty;
    let (open, closed) = (Some(BarrelOpenState::Open), Some(BarrelOpenState::Closed));
    assert_eq!(barrel.open(property).barrel_open, open);
    assert_eq!(barrel.open(property).barrel_open, open);
    assert_eq!(barrel.close(property).barrel_open, open);
    assert_eq!(barrel.close(property).barrel_open, closed);
}

#[test]
fn hot_paths_allocate_zero_bytes() {
    let pos = BlockPos::new(10, 64, -5);
    let (chest, north) = (ContainerKind::Chest, ContainerFacing::North);
    let neighbor = half(pos, chest, ChestType::Single);
    let mut lid = ContainerLidState::new();
    let before = allocations();
    let ok = classify_block("chest", ChestSpan::Double).is_some()
        && pair_on_place(BlockPos::new(9, 64, -5), chest, north, neighbor).is_some()
        && double_chest_slot(30).is_some()
        && obstructing_pos(pos, ContainerKind::ShulkerBox, north).is_some()
        && !can_insert_item_id(ContainerKind::ShulkerBox, "blue_shulker_box")
        && lid.open(LidFeedback::BlockEvent).sound == LidSoundTrigger::Open
        && lid.close(LidFeedback::BlockEvent).sound == LidSoundTrigger::Close;
    assert!(ok && allocations() == before, "zero alloc");
}
