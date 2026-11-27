use crate::container_kind::{ContainerKind, CopperChestVariant};
use crate::container_menu_kind::ContainerMenuKind;
use crate::lid_feedback::LidFeedback;
use crate::obstruction::{ObstructionRule, obstruction_rule};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LidSoundPair {
    pub open: &'static str,
    pub close: &'static str,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChestSpan {
    Single,
    Double,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ContainerLayout {
    pub kind: ContainerKind,
    pub slot_count: u8,
    pub menu: ContainerMenuKind,
    pub default_title_key: &'static str,
    pub lid_sounds: Option<LidSoundPair>,
    pub lid_feedback: LidFeedback,
    pub obstruction: ObstructionRule,
}

impl ContainerLayout {
    #[must_use]
    pub const fn new(
        kind: ContainerKind,
        menu: ContainerMenuKind,
        default_title_key: &'static str,
        lid_sounds: Option<LidSoundPair>,
        lid_feedback: LidFeedback,
        obstruction: ObstructionRule,
    ) -> Self {
        let slot_count = menu.slot_count();
        Self {
            kind,
            slot_count,
            menu,
            default_title_key,
            lid_sounds,
            lid_feedback,
            obstruction,
        }
    }
}

const fn lid_sounds_for(kind: ContainerKind) -> Option<LidSoundPair> {
    use ContainerKind::{
        Barrel, Chest, CopperChest, Dispenser, Dropper, EnderChest, Hopper, ShulkerBox,
        TrappedChest,
    };
    use CopperChestVariant::{Oxidized, WaxedOxidized, WaxedWeathered, Weathered};
    macro_rules! sound {
        ($name:literal) => {
            Some(LidSoundPair {
                open: concat!("minecraft:block.", $name, ".open"),
                close: concat!("minecraft:block.", $name, ".close"),
            })
        };
    }
    match kind {
        Chest | TrappedChest => sound!("chest"),
        CopperChest(Weathered | WaxedWeathered) => sound!("copper_chest_weathered"),
        CopperChest(Oxidized | WaxedOxidized) => sound!("copper_chest_oxidized"),
        CopperChest(_) => sound!("copper_chest"),
        Barrel => sound!("barrel"),
        ShulkerBox => sound!("shulker_box"),
        EnderChest => sound!("ender_chest"),
        Dispenser | Dropper | Hopper => None,
    }
}

#[must_use]
pub fn classify_container(kind: ContainerKind, span: ChestSpan) -> ContainerLayout {
    use ContainerKind::{
        Barrel, Chest, CopperChest, Dispenser, Dropper, EnderChest, Hopper, TrappedChest,
    };
    use ContainerMenuKind::{Generic3x3, Generic9x3, Generic9x6};
    let double = kind.supports_double_pairing() && matches!(span, ChestSpan::Double);
    let (menu, title) = match kind {
        Chest | TrappedChest | CopperChest(_) if double => (Generic9x6, "container.chestDouble"),
        Chest | TrappedChest | CopperChest(_) => (Generic9x3, "container.chest"),
        Barrel => (Generic9x3, "container.barrel"),
        ContainerKind::ShulkerBox => (ContainerMenuKind::ShulkerBox, "container.shulkerBox"),
        EnderChest => (Generic9x3, "container.enderchest"),
        Dispenser => (Generic3x3, "container.dispenser"),
        Dropper => (Generic3x3, "container.dropper"),
        Hopper => (ContainerMenuKind::Hopper, "container.hopper"),
    };
    let feedback = match kind {
        Barrel => LidFeedback::OpenProperty,
        Dispenser | Dropper | Hopper => LidFeedback::None,
        _ => LidFeedback::BlockEvent,
    };
    let sounds = lid_sounds_for(kind);
    ContainerLayout::new(kind, menu, title, sounds, feedback, obstruction_rule(kind))
}

#[must_use]
pub fn classify_block(block_id: &str, span: ChestSpan) -> Option<ContainerLayout> {
    let kind = ContainerKind::from_block_id(block_id)?;
    Some(classify_container(kind, span))
}
