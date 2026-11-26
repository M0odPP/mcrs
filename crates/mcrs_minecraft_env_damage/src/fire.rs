pub const PLAYER_FIRE_GRACE: i16 = -20;
pub const FIRE_TICK_INTERVAL: i16 = 20;
pub const IN_FIRE_IGNITE_TICKS: i16 = 160;
pub const LAVA_IGNITE_TICKS: i16 = 300;
pub const ON_FIRE_DAMAGE: f32 = 1.0;
pub const IN_FIRE_DAMAGE: f32 = 1.0;
pub const IN_SOUL_FIRE_DAMAGE: f32 = 2.0;
pub const LAVA_DAMAGE: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireImmunity {
    Vulnerable,
    Immune,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FireSource {
    None,
    InFire,
    InSoulFire,
    InLava,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WetExposure {
    Dry,
    ExtinguishedByWetness,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FireStepOutcome {
    pub next_fire_ticks: i16,
    pub extinguished: bool,
    pub burning_damage: Option<f32>,
    pub contact_damage: Option<f32>,
    pub contact_is_lava: bool,
}

crate::check_layout!(FireImmunity, 1, 1, "FireImmunity");
crate::check_layout!(FireSource, 1, 1, "FireSource");
crate::check_layout!(WetExposure, 1, 1, "WetExposure");
crate::check_layout!(FireStepOutcome, 20, 4, "FireStepOutcome");

#[must_use]
pub const fn ignite(current_ticks: i16, duration_ticks: i16) -> i16 {
    if duration_ticks > current_ticks {
        duration_ticks
    } else {
        current_ticks
    }
}

#[must_use]
pub const fn extinguish(current_ticks: i16) -> i16 {
    if current_ticks > 0 { 0 } else { current_ticks }
}

const fn normalize_grace(grace_ticks: i16) -> i16 {
    if grace_ticks > 0 {
        -grace_ticks
    } else {
        grace_ticks
    }
}

const fn step_in_fire(after_base: i16, vulnerable: bool) -> i16 {
    if !vulnerable {
        after_base
    } else if after_base <= 0 {
        let incremented = after_base.saturating_add(1);
        if incremented < 0 {
            incremented
        } else {
            IN_FIRE_IGNITE_TICKS
        }
    } else {
        ignite(after_base, IN_FIRE_IGNITE_TICKS)
    }
}

#[must_use]
pub const fn step_fire(
    current_ticks: i16,
    grace_ticks: i16,
    source: FireSource,
    wetness: WetExposure,
    immunity: FireImmunity,
) -> FireStepOutcome {
    let vulnerable = matches!(immunity, FireImmunity::Vulnerable);
    let interval_hit = current_ticks > 0 && current_ticks % FIRE_TICK_INTERVAL == 0;
    let burning_damage = if !matches!(source, FireSource::InLava) && vulnerable && interval_hit {
        Some(ON_FIRE_DAMAGE)
    } else {
        None
    };
    let after_base = if current_ticks > 0 {
        if vulnerable { current_ticks - 1 } else { 0 }
    } else {
        current_ticks
    };
    let (mut next_ticks, contact_damage, contact_is_lava, mut raised) = match source {
        FireSource::InLava => {
            let next = if vulnerable {
                ignite(after_base, LAVA_IGNITE_TICKS)
            } else {
                after_base
            };
            let damage = if vulnerable { Some(LAVA_DAMAGE) } else { None };
            (next, damage, vulnerable, vulnerable)
        }
        FireSource::InFire | FireSource::InSoulFire => {
            let next = step_in_fire(after_base, vulnerable);
            let damage = match (vulnerable, source) {
                (true, FireSource::InFire) => Some(IN_FIRE_DAMAGE),
                (true, FireSource::InSoulFire) => Some(IN_SOUL_FIRE_DAMAGE),
                _ => None,
            };
            (next, damage, false, vulnerable)
        }
        FireSource::None => (after_base, None, false, false),
    };
    let wet = matches!(wetness, WetExposure::ExtinguishedByWetness);
    let extinguished = wet && next_ticks > 0;
    if wet {
        next_ticks = 0;
        raised = false;
    }
    if next_ticks <= 0 && !raised {
        next_ticks = normalize_grace(grace_ticks);
    }
    FireStepOutcome {
        next_fire_ticks: next_ticks,
        extinguished,
        burning_damage,
        contact_damage,
        contact_is_lava,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fire_grace_ignite_extinguish_and_tick() {
        use FireImmunity::{Immune, Vulnerable};
        use WetExposure::{Dry, ExtinguishedByWetness};
        let ignited = ignite(PLAYER_FIRE_GRACE, IN_FIRE_IGNITE_TICKS) == 160;
        let cleared = extinguish(80) == 0 && extinguish(-12) == -12;
        let run = |ticks, source, wetness, immunity| {
            step_fire(ticks, PLAYER_FIRE_GRACE, source, wetness, immunity)
        };
        let dry = |ticks, source| run(ticks, source, Dry, Vulnerable);
        let twentieth = dry(-1, FireSource::InFire);
        let cooled = dry(200, FireSource::InFire);
        let burnout = dry(1, FireSource::None);
        let cooled_ok = cooled.next_fire_ticks == 199 && burnout.next_fire_ticks == -20;
        let core_ok = ignited && cleared && twentieth.next_fire_ticks == 160 && cooled_ok;
        assert!(core_ok, "fire ignites on 20th tick and resets");
        let wet = run(100, FireSource::InFire, ExtinguishedByWetness, Vulnerable);
        let wet_ok = wet.burning_damage == Some(1.0)
            && wet.contact_damage == Some(1.0)
            && wet.extinguished
            && wet.next_fire_ticks == -20;
        assert!(wet_ok, "fire in water hurts and extinguishes");
        let immune = run(PLAYER_FIRE_GRACE, FireSource::InLava, Dry, Immune);
        let safe_ok = immune.next_fire_ticks == -20
            && immune.contact_damage.is_none()
            && !immune.contact_is_lava;
        assert!(safe_ok, "fire-immune entity skips lava damage");
    }
}
