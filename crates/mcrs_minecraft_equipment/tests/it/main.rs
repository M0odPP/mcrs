use core::mem::{align_of, size_of};

use mcrs_minecraft_equipment::{EquipSlot, EquipmentChangeOrigin, EquipmentSlots, diff_equipment};
use mcrs_test_alloc::{CountingAllocator, allocations};

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stack(u16, u8, u16);

#[test]
fn equipment_slot_layout_and_wire_order_are_compact() {
    assert_eq!(
        (
            size_of::<EquipSlot>(),
            align_of::<EquipSlot>(),
            size_of::<EquipmentSlots<u8>>(),
            align_of::<EquipmentSlots<u8>>()
        ),
        (1, 1, 12, 1)
    );
    assert_eq!(EquipSlot::ALL.map(EquipSlot::wire_id), [0, 1, 2, 3, 4, 5]);
    assert_eq!(
        [
            EquipSlot::MainHand,
            EquipSlot::OffHand,
            EquipSlot::Feet,
            EquipSlot::Legs,
            EquipSlot::Chest,
            EquipSlot::Head
        ]
        .map(|slot| slot.player_slot(3)),
        [3, 40, 36, 37, 38, 39]
    );
    assert_eq!(EquipSlot::MainHand.player_slot(99), 0);
    assert_eq!(EquipSlot::from_player_slot(3, 3), Some(EquipSlot::MainHand));
    assert_eq!(EquipSlot::from_player_slot(2, 3), None);
    assert_eq!(EquipSlot::from_player_slot(40, 3), Some(EquipSlot::OffHand));
    assert!(
        (0..6).all(|id| EquipSlot::from_wire_id(id).is_some())
            && EquipSlot::from_wire_id(6).is_none(),
        "wire IDs 0..=5 must decode and ID 6 must be rejected"
    );
}

#[test]
fn diff_reports_count_and_patch_changes_and_suppresses_hand_swap() {
    let (mut synced, mut live) = (EquipmentSlots::new(), EquipmentSlots::new());
    let mut changes = Vec::with_capacity(EquipSlot::COUNT);
    for (slot, val) in [
        (EquipSlot::MainHand, Some(Stack(10, 1, 0))),
        (EquipSlot::MainHand, Some(Stack(10, 2, 0))),
        (EquipSlot::MainHand, Some(Stack(10, 2, 5))),
    ] {
        live.set(slot, val);
        diff_equipment(
            &mut synced,
            &live,
            EquipmentChangeOrigin::SlotUpdate,
            &mut changes,
        );
        assert_eq!(changes, [(slot, val)]);
    }
    live.swap_hands();
    diff_equipment(
        &mut synced,
        &live,
        EquipmentChangeOrigin::HandSwap,
        &mut changes,
    );
    assert!(
        changes.is_empty(),
        "hand swap must suppress mainhand and offhand diffs: {changes:?}"
    );
    assert_eq!(synced, live);

    live.set(EquipSlot::Head, Some(Stack(20, 1, 0)));
    diff_equipment(
        &mut synced,
        &live,
        EquipmentChangeOrigin::SlotUpdate,
        &mut changes,
    );
    assert_eq!(changes, [(EquipSlot::Head, Some(Stack(20, 1, 0)))]);

    live.swap_hands();
    diff_equipment(
        &mut synced,
        &live,
        EquipmentChangeOrigin::SlotUpdate,
        &mut changes,
    );
    assert_eq!(
        changes,
        [
            (EquipSlot::MainHand, Some(Stack(10, 2, 5))),
            (EquipSlot::OffHand, None)
        ]
    );
}

#[test]
fn diff_equipment_has_zero_steady_state_allocations_for_copy_stacks() {
    let (mut synced, mut live) = (EquipmentSlots::<u64>::new(), EquipmentSlots::<u64>::new());
    let mut out = Vec::with_capacity(EquipSlot::COUNT);
    live.set(EquipSlot::MainHand, Some(1));
    live.set(EquipSlot::Head, Some(2));
    diff_equipment(
        &mut synced,
        &live,
        EquipmentChangeOrigin::SlotUpdate,
        &mut out,
    );

    let before = allocations();
    live.set(EquipSlot::MainHand, Some(4));
    diff_equipment(
        &mut synced,
        &live,
        EquipmentChangeOrigin::SlotUpdate,
        &mut out,
    );
    live.swap_hands();
    diff_equipment(
        &mut synced,
        &live,
        EquipmentChangeOrigin::HandSwap,
        &mut out,
    );
    assert_eq!(allocations() - before, 0);
}
