use mcrs_minecraft_item::component::predicate::ItemPredicate;
use mcrs_minecraft_loot::slot::SlotSource;
use mcrs_minecraft_loot_condition::ContextParam;
use mcrs_minecraft_predicate::SlotRange;
use mcrs_minecraft_registries::Named;
use mcrs_minecraft_resource::{RegistryValue, ResourceLocationBuf};
use serde::Deserialize;

use crate::loot_table_file::TagName;
use crate::modifier_file::ContentsComponent;

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SlotSourceFile {
    Tag(TagName),
    Key(ResourceLocationBuf),
    Many(Vec<SlotSourceElement>),
    One(Box<SlotSourceObject>),
}

impl RegistryValue for SlotSourceFile {
    type Registry = SlotSource;
}

mcrs_minecraft_registries::resolve_as_is!(SlotSourceFile);

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SlotSourceElement {
    Key(ResourceLocationBuf),
    Inline(Box<SlotSourceObject>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SlotSourceSetFile {
    Tag(TagName),
    Many(Vec<SlotSourceElement>),
    One(SlotSourceElement),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum SlotSourceObject {
    #[serde(rename = "minecraft:group", alias = "group")]
    Group { terms: SlotSourceSetFile },
    #[serde(rename = "minecraft:filtered", alias = "filtered")]
    Filtered {
        slot_source: SlotSourceFile,
        item_filter: Box<ItemPredicate<Named>>,
    },
    #[serde(rename = "minecraft:limit_slots", alias = "limit_slots")]
    LimitSlots {
        slot_source: SlotSourceFile,
        limit: PositiveCount,
    },
    #[serde(rename = "minecraft:slot_range", alias = "slot_range")]
    SlotRange {
        slots: SlotRange,
        #[serde(default)]
        source: SlotSourceTarget,
    },
    #[serde(rename = "minecraft:contents", alias = "contents")]
    Contents {
        slot_source: SlotSourceFile,
        component: ContentsComponent,
    },
    #[serde(rename = "minecraft:empty", alias = "empty")]
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(try_from = "i32")]
pub struct PositiveCount(pub u32);

impl TryFrom<i32> for PositiveCount {
    type Error = &'static str;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        u32::try_from(value)
            .ok()
            .filter(|&count| count > 0)
            .map(Self)
            .ok_or("limit must be a positive integer")
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotSourceTarget {
    #[default]
    Container,
    This,
    InteractingEntity,
    TargetEntity,
    AttackingPlayer,
    Attacker,
    DirectAttacker,
    BlockEntity,
    Tool,
}

impl SlotSourceTarget {
    #[must_use]
    pub const fn context_param(self) -> ContextParam {
        match self {
            Self::Container => ContextParam::Container,
            Self::This => ContextParam::ThisEntity,
            Self::InteractingEntity => ContextParam::InteractingEntity,
            Self::TargetEntity => ContextParam::TargetEntity,
            Self::AttackingPlayer => ContextParam::LastDamagePlayer,
            Self::Attacker => ContextParam::AttackingEntity,
            Self::DirectAttacker => ContextParam::DirectAttackingEntity,
            Self::BlockEntity => ContextParam::BlockEntity,
            Self::Tool => ContextParam::Tool,
        }
    }
}
