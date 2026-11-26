use mcrs_minecraft_pose::Pose;
use mcrs_minecraft_protocol::VarInt;
use mcrs_minecraft_protocol_game::entity::{MetaDataValue, MetadataEntry};
use mcrs_minecraft_protocol_game::entity_data_index::{
    AIR_SUPPLY, LIVING_FLAGS, POSE, SHARED_FLAGS, TICKS_FROZEN,
};

use crate::mob_bundle::health_entry;

const DEFAULT_HEALTH: f32 = 1.0;
const DEFAULT_AIR_SUPPLY: i16 = 300;
const DEFAULT_TICKS_FROZEN: i32 = 0;

#[derive(Clone, Copy, Debug)]
pub struct PlayerEntries {
    pub shared_flags: u8,
    pub air_supply: i16,
    pub pose: Pose,
    pub ticks_frozen: i32,
    pub living_flags: u8,
    pub health: f32,
}

impl Default for PlayerEntries {
    fn default() -> Self {
        Self {
            shared_flags: 0,
            air_supply: DEFAULT_AIR_SUPPLY,
            pose: Pose::default(),
            ticks_frozen: DEFAULT_TICKS_FROZEN,
            living_flags: 0,
            health: DEFAULT_HEALTH,
        }
    }
}

impl PartialEq for PlayerEntries {
    fn eq(&self, other: &Self) -> bool {
        self.shared_flags == other.shared_flags
            && self.air_supply == other.air_supply
            && self.pose == other.pose
            && self.ticks_frozen == other.ticks_frozen
            && self.living_flags == other.living_flags
            && self.health.to_bits() == other.health.to_bits()
    }
}

impl PlayerEntries {
    #[must_use]
    pub fn changed_from(&self, sent: &Self) -> Vec<MetadataEntry<'static>> {
        let mut entries = Vec::new();
        if self.shared_flags != sent.shared_flags {
            entries.push(MetadataEntry {
                index: SHARED_FLAGS,
                value: MetaDataValue::Byte(self.shared_flags.cast_signed()),
            });
        }
        if self.air_supply != sent.air_supply {
            entries.push(MetadataEntry {
                index: AIR_SUPPLY,
                value: MetaDataValue::VarInt(VarInt(i32::from(self.air_supply))),
            });
        }
        if self.pose != sent.pose {
            entries.push(MetadataEntry {
                index: POSE,
                value: MetaDataValue::Pose(self.pose),
            });
        }
        if self.ticks_frozen != sent.ticks_frozen {
            entries.push(MetadataEntry {
                index: TICKS_FROZEN,
                value: MetaDataValue::VarInt(VarInt(self.ticks_frozen)),
            });
        }
        if self.living_flags != sent.living_flags {
            entries.push(MetadataEntry {
                index: LIVING_FLAGS,
                value: MetaDataValue::Byte(self.living_flags.cast_signed()),
            });
        }
        if self.health.to_bits() != sent.health.to_bits() {
            entries.push(health_entry(self.health));
        }
        entries
    }
}

#[cfg(test)]
mod tests {
    use mcrs_minecraft_protocol_game::entity_data_index::HEALTH;

    use super::*;

    #[test]
    fn changed_from_emits_fire_air_and_frozen_in_index_order() {
        let updated = PlayerEntries {
            shared_flags: 1,
            air_supply: 280,
            pose: Pose::Crouching,
            ticks_frozen: 42,
            living_flags: 1,
            health: 15.0,
        };
        let entries = updated.changed_from(&PlayerEntries::default());
        let indices: Vec<u8> = entries.iter().map(|entry| entry.index).collect();
        assert_eq!(
            indices,
            vec![
                SHARED_FLAGS,
                AIR_SUPPLY,
                POSE,
                TICKS_FROZEN,
                LIVING_FLAGS,
                HEALTH,
            ],
            "changed_from should emit metadata indices in ascending order"
        );
        assert_eq!(
            entries[1].value,
            MetaDataValue::VarInt(VarInt(280)),
            "air supply entry should encode VarInt air value"
        );
        assert_eq!(
            entries[3].value,
            MetaDataValue::VarInt(VarInt(42)),
            "ticks frozen entry should encode VarInt ticks value"
        );
        assert_eq!(
            entries[4].value,
            MetaDataValue::Byte(1),
            "living flags entry should encode Byte flags value"
        );
        assert!(
            updated.changed_from(&updated).is_empty(),
            "unchanged entries should emit empty delta"
        );
    }
}
