pub const PLAYER_MAX_AIR: i16 = 300;
pub const DROWN_TRIGGER_AIR: i16 = -20;
pub const DROWN_RESET_AIR: i16 = 0;
pub const AIR_REGEN_PER_TICK: i16 = 4;
pub const DROWN_DAMAGE: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreathableMedium {
    AirOrWaterBreathing,
    Underwater,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirStepOutcome {
    pub next_air: i16,
    pub damage: Option<f32>,
    pub emit_drown_particles: bool,
}

crate::check_layout!(BreathableMedium, 1, 1, "BreathableMedium");
crate::check_layout!(AirStepOutcome, 12, 4, "AirStepOutcome");

#[must_use]
pub const fn step_air(current_air: i16, max_air: i16, medium: BreathableMedium) -> AirStepOutcome {
    match medium {
        BreathableMedium::AirOrWaterBreathing => {
            let gained = current_air.saturating_add(AIR_REGEN_PER_TICK);
            let next_air = if current_air >= max_air || gained > max_air {
                max_air
            } else {
                gained
            };
            AirStepOutcome {
                next_air,
                damage: None,
                emit_drown_particles: false,
            }
        }
        BreathableMedium::Underwater => {
            let decreased = current_air.saturating_sub(1);
            if decreased <= DROWN_TRIGGER_AIR {
                AirStepOutcome {
                    next_air: DROWN_RESET_AIR,
                    damage: Some(DROWN_DAMAGE),
                    emit_drown_particles: true,
                }
            } else {
                AirStepOutcome {
                    next_air: decreased,
                    damage: None,
                    emit_drown_particles: false,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn air_depletes_drowns_and_regenerates() {
        let low = step_air(10, PLAYER_MAX_AIR, BreathableMedium::Underwater);
        let drown = step_air(-19, PLAYER_MAX_AIR, BreathableMedium::Underwater);
        let up = step_air(298, PLAYER_MAX_AIR, BreathableMedium::AirOrWaterBreathing);
        let low_ok = low.next_air == 9 && low.damage.is_none();
        let up_ok = up.next_air == 300 && up.damage.is_none();
        let drown_ok =
            drown.next_air == 0 && drown.damage == Some(2.0) && drown.emit_drown_particles;
        assert!(low_ok && up_ok, "air depletes and clamps regen");
        assert!(drown_ok, "drowning at -20 resets air and hurts");
    }
}
