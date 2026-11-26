use bevy_ecs::change_detection::DetectChangesMut;
use bevy_ecs::system::Res;
use bevy_ecs::system::ResMut;
use mcrs_minecraft_damage::DamageTags;
use mcrs_minecraft_damage::DamageTypeRules;
use mcrs_minecraft_entity::damage_type::DamageType;
use mcrs_minecraft_entity::damage_type_keys;
use mcrs_minecraft_entity::damage_type_tag_keys;
use mcrs_minecraft_entity::registry_keys::DAMAGE_TYPE;
use mcrs_minecraft_registries::CurrentRegistries;
use mcrs_minecraft_resource::TagKey;

use crate::DamageTypeIds;

struct TagBit {
    tag: TagKey<'static, DamageType>,
    bit: DamageTags,
}

const TAGS: [TagBit; 10] = [
    TagBit {
        tag: damage_type_tag_keys::BYPASSES_ARMOR,
        bit: DamageTags::BYPASSES_ARMOR,
    },
    TagBit {
        tag: damage_type_tag_keys::BYPASSES_COOLDOWN,
        bit: DamageTags::BYPASSES_COOLDOWN,
    },
    TagBit {
        tag: damage_type_tag_keys::BYPASSES_EFFECTS,
        bit: DamageTags::BYPASSES_EFFECTS,
    },
    TagBit {
        tag: damage_type_tag_keys::BYPASSES_RESISTANCE,
        bit: DamageTags::BYPASSES_RESISTANCE,
    },
    TagBit {
        tag: damage_type_tag_keys::IS_FIRE,
        bit: DamageTags::IS_FIRE,
    },
    TagBit {
        tag: damage_type_tag_keys::IS_FALL,
        bit: DamageTags::IS_FALL,
    },
    TagBit {
        tag: damage_type_tag_keys::NO_KNOCKBACK,
        bit: DamageTags::NO_KNOCKBACK,
    },
    TagBit {
        tag: damage_type_tag_keys::BYPASSES_INVULNERABILITY,
        bit: DamageTags::BYPASSES_INVULNERABILITY,
    },
    TagBit {
        tag: damage_type_tag_keys::IS_DROWNING,
        bit: DamageTags::IS_DROWNING,
    },
    TagBit {
        tag: damage_type_tag_keys::IS_FREEZING,
        bit: DamageTags::IS_FREEZING,
    },
];

pub(crate) fn bake_damage_types(
    registries: Res<CurrentRegistries>,
    mut rules: ResMut<DamageTypeRules>,
    mut ids: ResMut<DamageTypeIds>,
) {
    let count = registries
        .entries::<DamageType>()
        .map_or(0, |entries| entries.len());
    let mut rows = vec![DamageTags::empty(); count];
    if let Some(tags) = registries.tags::<DamageType>() {
        for TagBit { tag, bit } in TAGS {
            let Some(tag) = tags.id(tag) else {
                continue;
            };
            for member in tags.members(tag) {
                if let Some(row) = rows.get_mut(member.index()) {
                    *row = row.union(bit);
                }
            }
        }
    }
    let baked = match registries.registry::<DamageType>() {
        Some(registry) => DamageTypeRules::new(rows).with_exhaustions(
            registry
                .values()
                .iter()
                .map(|damage_type| damage_type.exhaustion),
        ),
        None => DamageTypeRules::new(rows),
    };
    if *rules != baked {
        *rules = baked;
    }
    let registry_key = DAMAGE_TYPE.location();
    let baked_ids = DamageTypeIds {
        starve: registries
            .id_of(registry_key, damage_type_keys::STARVE.location())
            .map(u32::from),
        player_attack: registries
            .id_of(registry_key, damage_type_keys::PLAYER_ATTACK.location())
            .map(u32::from),
    };
    ids.set_if_neq(baked_ids);
}
