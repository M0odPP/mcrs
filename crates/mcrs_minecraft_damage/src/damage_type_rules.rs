use alloc::vec::Vec;

use crate::DamageTags;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::resource::Resource))]
pub struct DamageTypeRules {
    rows: Vec<DamageTags>,
    exhaustions: Vec<u32>,
}

impl DamageTypeRules {
    #[must_use]
    pub fn new(rows: impl IntoIterator<Item = DamageTags>) -> Self {
        Self {
            rows: rows.into_iter().collect(),
            exhaustions: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_exhaustions(mut self, exhaustions: impl IntoIterator<Item = f32>) -> Self {
        self.exhaustions = exhaustions
            .into_iter()
            .map(|value| {
                if value.is_finite() && value >= 0.0 {
                    value.to_bits()
                } else {
                    0.0_f32.to_bits()
                }
            })
            .collect();
        self
    }

    #[must_use]
    pub fn tags(&self, damage_type: u32) -> DamageTags {
        usize::try_from(damage_type)
            .ok()
            .and_then(|index| self.rows.get(index))
            .copied()
            .unwrap_or_default()
    }

    #[must_use]
    pub fn exhaustion(&self, damage_type: u32) -> f32 {
        usize::try_from(damage_type)
            .ok()
            .and_then(|index| self.exhaustions.get(index))
            .copied()
            .map_or(0.0, f32::from_bits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exhaustion_defaults_to_zero_and_sanitizes_invalid_values() {
        let rules = DamageTypeRules::new([DamageTags::empty(), DamageTags::IS_FIRE])
            .with_exhaustions([0.1, f32::NAN, -1.0, f32::INFINITY]);
        assert_eq!(rules.exhaustion(0), 0.1);
        assert_eq!(rules.exhaustion(1), 0.0);
        assert_eq!(rules.exhaustion(2), 0.0);
        assert_eq!(rules.exhaustion(3), 0.0);
        assert_eq!(rules.exhaustion(99), 0.0);
    }
}
