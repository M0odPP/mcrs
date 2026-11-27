#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CopperChestVariant {
    Unaffected,
    Exposed,
    Weathered,
    Oxidized,
    WaxedUnaffected,
    WaxedExposed,
    WaxedWeathered,
    WaxedOxidized,
}

#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContainerKind {
    Chest,
    TrappedChest,
    CopperChest(CopperChestVariant),
    Barrel,
    ShulkerBox,
    EnderChest,
    Dispenser,
    Dropper,
    Hopper,
}

const SHULKER_COLORS: &str = concat!(
    "white,orange,magenta,light_blue,yellow,lime,pink,gray,",
    "light_gray,cyan,purple,blue,brown,green,red,black"
);

fn strip_namespace(id: &str) -> &str {
    id.strip_prefix("minecraft:").unwrap_or(id)
}

#[must_use]
pub fn is_shulker_box_id(id: &str) -> bool {
    let name = strip_namespace(id);
    name == "shulker_box"
        || name
            .strip_suffix("_shulker_box")
            .is_some_and(|color| SHULKER_COLORS.split(',').any(|entry| entry == color))
}

const fn copper(variant: CopperChestVariant) -> Option<ContainerKind> {
    Some(ContainerKind::CopperChest(variant))
}

impl ContainerKind {
    #[must_use]
    pub fn from_block_id(id: &str) -> Option<Self> {
        let name = strip_namespace(id);
        match name {
            "chest" => Some(Self::Chest),
            "trapped_chest" => Some(Self::TrappedChest),
            "copper_chest" => copper(CopperChestVariant::Unaffected),
            "exposed_copper_chest" => copper(CopperChestVariant::Exposed),
            "weathered_copper_chest" => copper(CopperChestVariant::Weathered),
            "oxidized_copper_chest" => copper(CopperChestVariant::Oxidized),
            "waxed_copper_chest" => copper(CopperChestVariant::WaxedUnaffected),
            "waxed_exposed_copper_chest" => copper(CopperChestVariant::WaxedExposed),
            "waxed_weathered_copper_chest" => copper(CopperChestVariant::WaxedWeathered),
            "waxed_oxidized_copper_chest" => copper(CopperChestVariant::WaxedOxidized),
            "barrel" => Some(Self::Barrel),
            "ender_chest" => Some(Self::EnderChest),
            "dispenser" => Some(Self::Dispenser),
            "dropper" => Some(Self::Dropper),
            "hopper" => Some(Self::Hopper),
            _ if is_shulker_box_id(name) => Some(Self::ShulkerBox),
            _ => None,
        }
    }

    #[must_use]
    pub fn from_block_entity_id(id: &str) -> Option<Self> {
        Self::from_block_id(id).filter(|kind| !matches!(kind, Self::CopperChest(_)))
    }

    #[must_use]
    pub const fn supports_double_pairing(self) -> bool {
        matches!(
            self,
            Self::Chest | Self::TrappedChest | Self::CopperChest(_)
        )
    }
}
