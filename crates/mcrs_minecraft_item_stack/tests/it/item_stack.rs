use mcrs_minecraft_item_stack::BeeOccupant;
use mcrs_minecraft_item_stack::Bees;
use mcrs_minecraft_item_stack::BreakBehavior;
use mcrs_minecraft_item_stack::Builtin;
use mcrs_minecraft_item_stack::Count;
use mcrs_minecraft_item_stack::CustomComponent;
use mcrs_minecraft_item_stack::CustomData;
use mcrs_minecraft_item_stack::Damage;
use mcrs_minecraft_item_stack::DamageOutcome;
use mcrs_minecraft_item_stack::EntityData;
use mcrs_minecraft_item_stack::EntityTypeId;
use mcrs_minecraft_item_stack::ItemDefaults;
use mcrs_minecraft_item_stack::ItemStack;
use mcrs_minecraft_item_stack::Kind;
use mcrs_minecraft_item_stack::MaxDamage;
use mcrs_minecraft_item_stack::Rarity;
use mcrs_minecraft_item_stack::RepairCost;
use mcrs_minecraft_item_stack::Unbreakable;
use mcrs_minecraft_item_stack::Weapon;
use mcrs_minecraft_nbt::compound::NbtCompound;
use mcrs_minecraft_nbt::tag::NbtTag;

#[derive(Clone, PartialEq, Hash)]
struct Owner(u8);

impl CustomComponent for Owner {
    const KEY: &'static str = "test:owner";

    fn encode(&self, bytes: &mut Vec<u8>) {
        bytes.push(self.0);
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [owner] => Some(Self(*owner)),
            _ => None,
        }
    }
}

#[derive(Clone, PartialEq, Hash)]
struct Batch(u8);

impl CustomComponent for Batch {
    const KEY: &'static str = "test:batch";

    fn encode(&self, bytes: &mut Vec<u8>) {
        bytes.push(self.0);
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [batch] => Some(Self(*batch)),
            _ => None,
        }
    }
}

fn compound(entries: &[(&str, NbtTag)]) -> NbtCompound {
    entries
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

fn bee(data: NbtCompound) -> Bees {
    Bees(vec![BeeOccupant {
        entity_data: EntityData::new(EntityTypeId(4), data),
        ticks_in_hive: 0,
        min_ticks_in_hive: 0,
    }])
}

fn stack(kind: u16, count: u8) -> ItemStack {
    let count = Count::new(count).expect("the test counts are not zero");
    ItemStack::new(Kind(kind), count)
}

#[test]
fn stacks_are_equal_when_item_count_and_patch_entries_are_equal() {
    let plain = || stack(7, 1);
    let one = NbtTag::Int(1);
    let half = NbtTag::Float(0.5);
    let xy = NbtTag::Compound(compound(&[("x", one.clone()), ("y", half.clone())]));
    let yx = NbtTag::Compound(compound(&[("y", half), ("x", one.clone())]));
    let cases: [(&str, ItemStack, ItemStack, bool); 14] = [
        (
            "the order of two built-in values has no effect",
            plain().set(Damage(12)).set(Rarity::Epic),
            plain().set(Rarity::Epic).set(Damage(12)),
            true,
        ),
        (
            "the order of a value and a removal has no effect",
            plain().unset::<RepairCost>().set(Unbreakable),
            plain().set(Unbreakable).unset::<RepairCost>(),
            true,
        ),
        (
            "the order of built-in and custom values has no effect",
            plain().set(Owner(3)).set(Damage(1)).set(Batch(9)),
            plain().set(Batch(9)).set(Owner(3)).set(Damage(1)),
            true,
        ),
        (
            "the last value of a type replaces the earlier one",
            plain().set(Damage(1)).set(Damage(2)),
            plain().set(Damage(2)),
            true,
        ),
        (
            "a removal replaces the value of the type",
            plain().set(Damage(1)).unset::<Damage>(),
            plain().unset::<Damage>(),
            true,
        ),
        (
            "the removal of a custom component leaves no entry",
            plain().set(Owner(3)).unset::<Owner>(),
            plain(),
            true,
        ),
        (
            "the key order of custom data has no effect at any depth",
            plain().set(CustomData(compound(&[
                ("a", one.clone()),
                ("b", xy.clone()),
            ]))),
            plain().set(CustomData(compound(&[
                ("b", yx.clone()),
                ("a", one.clone()),
            ]))),
            true,
        ),
        (
            "the key order of bee entity data has no effect at any depth",
            plain().set(bee(compound(&[("a", one.clone()), ("b", xy)]))),
            plain().set(bee(compound(&[("b", yx), ("a", one)]))),
            true,
        ),
        (
            "different values of one type",
            plain().set(Damage(1)),
            plain().set(Damage(2)),
            false,
        ),
        (
            "different values of one custom type",
            plain().set(Owner(1)),
            plain().set(Owner(2)),
            false,
        ),
        (
            "a removal entry against no entry",
            plain().unset::<Damage>(),
            plain(),
            false,
        ),
        (
            "a removal entry against a value entry",
            plain().unset::<Damage>(),
            plain().set(Damage(0)),
            false,
        ),
        (
            "different items",
            stack(7, 1).set(Damage(1)),
            stack(8, 1).set(Damage(1)),
            false,
        ),
        (
            "different counts",
            stack(7, 1).set(Damage(1)),
            stack(7, 2).set(Damage(1)),
            false,
        ),
    ];
    for (name, left, right, equal) in cases {
        assert_eq!(left == right, equal, "{name}");
    }
}

#[test]
fn stacks_are_mergeable_when_item_and_patch_are_equal() {
    let cases: [(&str, ItemStack, ItemStack, bool); 4] = [
        (
            "different counts",
            stack(7, 1).set(Rarity::Rare).unset::<Damage>(),
            stack(7, 64).unset::<Damage>().set(Rarity::Rare),
            true,
        ),
        ("different items", stack(7, 1), stack(8, 1), false),
        (
            "different values of one type",
            stack(7, 1).set(Damage(1)),
            stack(7, 1).set(Damage(2)),
            false,
        ),
        (
            "a custom value against no entry",
            stack(7, 1).set(Owner(3)),
            stack(7, 1),
            false,
        ),
    ];
    for (name, left, right, mergeable) in cases {
        assert_eq!(left.mergeable(&right), mergeable, "{name}");
    }
}

struct ProtoDefaults<'a>(&'a [Builtin]);

impl ItemDefaults for ProtoDefaults<'_> {
    fn default_components(&self, _kind: Kind) -> &[Builtin] {
        self.0
    }
}

fn tool_defaults() -> ProtoDefaults<'static> {
    ProtoDefaults(&[
        Builtin::MaxDamage(MaxDamage(32)),
        Builtin::Damage(Damage(0)),
    ])
}

#[test]
fn durability_predicates_and_defaults() {
    let d = tool_defaults();
    let fresh = stack(7, 1);
    let unbrk = fresh.clone().set(Unbreakable);
    let no_dmg = fresh.clone().unset::<Damage>();
    let no_max = fresh.clone().unset::<MaxDamage>();
    assert!(fresh.is_damageable(&d), "fresh tool is damageable");
    assert_eq!(fresh.max_damage(&d), 32, "max_damage default");
    assert_eq!(fresh.damage_value(&d), 0, "initial damage_value");
    assert!(!fresh.is_damaged(&d), "fresh stack is not damaged");
    assert!(!fresh.is_broken(&d), "fresh stack is not broken");
    assert!(!fresh.next_damage_will_break(&d), "fresh will not break");
    assert!(!unbrk.is_damageable(&d), "unbreakable not damageable");
    assert!(!no_dmg.is_damageable(&d), "no Damage not damageable");
    assert!(!no_max.is_damageable(&d), "no MaxDamage not damageable");
    assert!(!no_dmg.is_damaged(&d), "no Damage not damaged");
}

#[test]
fn durability_damage_lifecycle_and_normalization() {
    use BreakBehavior::{BreakAtMax, ClampBelowMax};
    use DamageOutcome::{Broken, Damaged, Unchanged};

    let d = tool_defaults();
    let fresh = stack(7, 1);

    let (damaged, out) = fresh.clone().apply_damage(10, BreakAtMax, &d);
    let damaged = damaged.expect("survives below max");
    assert_eq!(out, Damaged(10), "damaged outcome");
    assert!(damaged.is_damaged(&d), "damaged stack reports is_damaged");
    assert_eq!(damaged.damage_value(&d), 10, "updated damage_value");

    let (repaired, out) = damaged.apply_damage(-20, BreakAtMax, &d);
    let repaired = repaired.expect("repair returns to zero");
    assert_eq!(out, Damaged(0), "repaired outcome");
    assert!(repaired.patch().is_empty(), "repair normalizes Damage(0)");

    let (clamped, out) = fresh.clone().apply_damage(99, ClampBelowMax, &d);
    let clamped = clamped.expect("clamp keeps alive");
    assert_eq!(out, Damaged(31), "clamped to max - 1");
    assert!(clamped.next_damage_will_break(&d), "max - 1 breaks next");
    assert!(!clamped.is_broken(&d), "max - 1 is not broken");

    let (same, out) = clamped.clone().apply_damage(5, ClampBelowMax, &d);
    assert_eq!(out, Unchanged, "clamp at ceiling is Unchanged");
    assert_eq!(same.map(|s| s.damage_value(&d)), Some(31), "stays 31");

    let (broken, out) = clamped.apply_damage(1, BreakAtMax, &d);
    assert_eq!(broken, None, "single stack breaks at max");
    assert_eq!(out, Broken(Kind(7)), "broken outcome");

    let (rem, out) = stack(7, 3).set(Damage(31)).apply_damage(1, BreakAtMax, &d);
    let rem = rem.expect("stacked decrements");
    assert_eq!((rem.count.get(), rem.damage_value(&d)), (2, 32), "count=2");
    assert_eq!(out, Broken(Kind(7)), "stacked break outcome");

    let unbrk = fresh.set(Unbreakable);
    let (same, out) = unbrk.clone().apply_damage(5, BreakAtMax, &d);
    assert_eq!(out, Unchanged, "unbreakable Unchanged");
    assert_eq!(same, Some(unbrk), "unbreakable stack untouched");

    let at_max = stack(7, 1).set(Damage(32));
    assert!(at_max.is_broken(&d), "Damage(max) is broken");
    for b in [BreakAtMax, ClampBelowMax] {
        let (same, out) = at_max.clone().apply_damage(0, b, &d);
        assert_eq!(out, Unchanged, "STK-DMG-005 0-dmg Unchanged ({b:?})");
        assert_eq!(
            same.as_ref(),
            Some(&at_max),
            "STK-DMG-005 keeps max ({b:?})"
        );
    }
}

#[test]
fn weapon_equality_distinguishes_signed_zero() {
    let pos = Weapon {
        item_damage_per_attack: 1,
        disable_blocking_for_seconds: 0.0,
    };
    let neg = Weapon {
        item_damage_per_attack: 1,
        disable_blocking_for_seconds: -0.0,
    };
    assert_ne!(pos, neg, "Weapon equality must distinguish signed zero");
    assert_eq!(Weapon::new(), pos, "Weapon::new defaults match 1 / 0.0");
}
