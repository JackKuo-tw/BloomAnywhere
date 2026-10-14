use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CooldownView {
    pub last_distance_km: f64,
    pub total_secs: u64,
    pub until_unix_ms: u64,
    pub remaining_secs: u64,
}

pub fn calculate_cooldown_secs(distance_km: f64) -> u64 {
    let res = std::panic::catch_unwind(|| lc_core::cooldown::cooldown_secs(distance_km));
    if let Ok(secs) = res {
        return secs;
    }

    if distance_km < 1.0 {
        return 0;
    }

    const TABLE: &[(f64, u64)] = &[
        (1.0, 30),
        (2.0, 60),
        (3.0, 120),
        (5.0, 180),
        (10.0, 420),
        (25.0, 660),
        (50.0, 1200),
        (100.0, 1800),
        (250.0, 2700),
        (500.0, 3600),
        (750.0, 4500),
        (1000.0, 5400),
        (1350.0, 7200),
    ];

    for &(dist, secs) in TABLE {
        if distance_km <= dist {
            return secs;
        }
    }
    7200
}

pub fn create_cooldown(distance_km: f64, now_unix_ms: u64) -> Option<CooldownView> {
    let total_secs = calculate_cooldown_secs(distance_km);
    if total_secs == 0 {
        return None;
    }
    let until_unix_ms = now_unix_ms + total_secs * 1000;
    Some(CooldownView {
        last_distance_km: distance_km,
        total_secs,
        until_unix_ms,
        remaining_secs: total_secs,
    })
}

pub fn update_cooldown(status: &mut CooldownView, now_unix_ms: u64) -> bool {
    if now_unix_ms >= status.until_unix_ms {
        status.remaining_secs = 0;
        false // finished
    } else {
        let diff_ms = status.until_unix_ms - now_unix_ms;
        status.remaining_secs = diff_ms.div_ceil(1000);
        true // active
    }
}
