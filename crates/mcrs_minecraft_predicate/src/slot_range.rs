use core::str::FromStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SlotRange {
    Run { first: u16, len: u16 },
    Armor,
}

struct IndexedName {
    prefix: &'static str,
    first: u16,
    count: u16,
}

struct SingleName {
    name: &'static str,
    slot: u16,
}

const INDEXED: [IndexedName; 7] = [
    IndexedName {
        prefix: "container.",
        first: 0,
        count: 54,
    },
    IndexedName {
        prefix: "hotbar.",
        first: 0,
        count: 9,
    },
    IndexedName {
        prefix: "inventory.",
        first: 9,
        count: 27,
    },
    IndexedName {
        prefix: "enderchest.",
        first: 200,
        count: 27,
    },
    IndexedName {
        prefix: "mob.inventory.",
        first: 300,
        count: 8,
    },
    IndexedName {
        prefix: "horse.",
        first: 500,
        count: 15,
    },
    IndexedName {
        prefix: "player.crafting.",
        first: 500,
        count: 4,
    },
];

const SINGLE: [SingleName; 12] = [
    SingleName {
        name: "contents",
        slot: 0,
    },
    SingleName {
        name: "weapon",
        slot: 98,
    },
    SingleName {
        name: "weapon.mainhand",
        slot: 98,
    },
    SingleName {
        name: "weapon.offhand",
        slot: 99,
    },
    SingleName {
        name: "armor.feet",
        slot: 100,
    },
    SingleName {
        name: "armor.legs",
        slot: 101,
    },
    SingleName {
        name: "armor.chest",
        slot: 102,
    },
    SingleName {
        name: "armor.head",
        slot: 103,
    },
    SingleName {
        name: "armor.body",
        slot: 105,
    },
    SingleName {
        name: "saddle",
        slot: 106,
    },
    SingleName {
        name: "horse.chest",
        slot: 499,
    },
    SingleName {
        name: "player.cursor",
        slot: 499,
    },
];

const ARMOR: [u16; 5] = [100, 101, 102, 103, 105];

impl SlotRange {
    #[must_use = "iterators are lazy and do nothing unless consumed"]
    pub fn slots(self) -> impl Iterator<Item = u16> {
        match self {
            Self::Run { first, len } => SlotIterator::Run(first..first.saturating_add(len)),
            Self::Armor => SlotIterator::Armor(ARMOR.into_iter()),
        }
    }

    pub(crate) fn any_slot(self, test: impl FnMut(u16) -> bool) -> bool {
        match self {
            SlotRange::Run { first, len } => (first..first.saturating_add(len)).any(test),
            SlotRange::Armor => ARMOR.into_iter().any(test),
        }
    }
}

enum SlotIterator {
    Run(core::ops::Range<u16>),
    Armor(core::array::IntoIter<u16, 5>),
}

impl Iterator for SlotIterator {
    type Item = u16;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Run(range) => range.next(),
            Self::Armor(armor) => armor.next(),
        }
    }
}

impl<'de> serde::Deserialize<'de> for SlotRange {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;

        impl serde::de::Visitor<'_> for Visitor {
            type Value = SlotRange;

            fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                formatter.write_str("a slot range string")
            }

            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(serde::de::Error::custom)
            }
        }

        deserializer.deserialize_str(Visitor)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownSlotRange;

impl core::fmt::Display for UnknownSlotRange {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str("unknown slot range")
    }
}

impl core::error::Error for UnknownSlotRange {}

impl FromStr for SlotRange {
    type Err = UnknownSlotRange;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "weapon.*" => return Ok(SlotRange::Run { first: 98, len: 2 }),
            "armor.*" => return Ok(SlotRange::Armor),
            _ => {}
        }
        if let Some(single) = SINGLE.iter().find(|single| single.name == text) {
            return Ok(SlotRange::Run {
                first: single.slot,
                len: 1,
            });
        }
        for IndexedName {
            prefix,
            first,
            count,
        } in INDEXED
        {
            let Some(index) = text.strip_prefix(prefix) else {
                continue;
            };
            if index == "*" {
                return Ok(SlotRange::Run { first, len: count });
            }
            let canonical = index.bytes().all(|byte| byte.is_ascii_digit())
                && (index == "0" || !index.starts_with('0'));
            if let Ok(index) = index.parse::<u16>()
                && canonical
                && index < count
            {
                return Ok(SlotRange::Run {
                    first: first + index,
                    len: 1,
                });
            }
        }
        Err(UnknownSlotRange)
    }
}
