//! Teleport cooldown table based on location-based game safety standards.
use serde::{Deserialize, Serialize};

/// Recommended wait time in seconds after jumping `distance_km`.
/// Must be monotonic non-decreasing, 0 for < 1 km, capped at 2 hours (7200 s) for >= ~1300 km.
pub fn cooldown_secs(distance_km: f64) -> u64 {
    if !distance_km.is_finite() || distance_km < 1.0 {
        return 0;
    }

    const TABLE: &[(f64, u64)] = &[
        (2.0, 60),
        (4.0, 120),
        (5.0, 120),
        (6.0, 180),
        (7.0, 300),
        (8.0, 360),
        (10.0, 420),
        (12.0, 480),
        (15.0, 660),
        (18.0, 720),
        (20.0, 720),
        (25.0, 900),
        (30.0, 1080),
        (40.0, 1320),
        (45.0, 1380),
        (60.0, 1500),
        (80.0, 1620),
        (100.0, 1800),
        (125.0, 1980),
        (150.0, 2160),
        (180.0, 2340),
        (200.0, 2460),
        (250.0, 2700),
        (300.0, 3000),
        (350.0, 3120),
        (400.0, 3300),
        (500.0, 3780),
        (600.0, 4200),
        (700.0, 4680),
        (800.0, 4920),
        (900.0, 5460),
        (1000.0, 5880),
        (1250.0, 7020),
    ];

    for &(max_km, secs) in TABLE {
        if distance_km <= max_km {
            return secs;
        }
    }
    7200
}

/// Snapshot of an active cooldown, serialised to the web UI.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CooldownStatus {
    pub last_distance_km: f64,
    pub total_secs: u64,
    /// Unix epoch milliseconds at which the cooldown ends.
    pub until_unix_ms: u64,
}

impl CooldownStatus {
    pub fn new(distance_km: f64, now_unix_ms: u64) -> Self {
        let total_secs = cooldown_secs(distance_km);
        let until_unix_ms = now_unix_ms.saturating_add(total_secs.saturating_mul(1000));
        Self {
            last_distance_km: distance_km,
            total_secs,
            until_unix_ms,
        }
    }

    pub fn remaining_secs(&self, now_unix_ms: u64) -> u64 {
        self.until_unix_ms.saturating_sub(now_unix_ms) / 1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cooldown_bounds() {
        assert_eq!(cooldown_secs(-5.0), 0);
        assert_eq!(cooldown_secs(0.0), 0);
        assert_eq!(cooldown_secs(0.999), 0);
        assert_eq!(cooldown_secs(f64::NAN), 0);

        assert_eq!(cooldown_secs(1.0), 60);
        assert_eq!(cooldown_secs(2.0), 60);
        assert_eq!(cooldown_secs(4.0), 120);
        assert_eq!(cooldown_secs(5.0), 120);
        assert_eq!(cooldown_secs(10.0), 420);
        assert_eq!(cooldown_secs(100.0), 1800);
        assert_eq!(cooldown_secs(1250.0), 7020);
        assert_eq!(cooldown_secs(1250.1), 7200);
        assert_eq!(cooldown_secs(2000.0), 7200);
    }

    #[test]
    fn test_cooldown_monotonic() {
        let mut prev = 0;
        let mut d = 0.0;
        while d <= 2000.0 {
            let secs = cooldown_secs(d);
            assert!(
                secs >= prev,
                "cooldown at {d} km ({secs}s) < previous ({prev}s)"
            );
            prev = secs;
            d += 0.5;
        }
    }

    #[test]
    fn test_cooldown_status() {
        let now = 1_700_000_000_000u64;
        let status = CooldownStatus::new(10.0, now);
        assert_eq!(status.last_distance_km, 10.0);
        assert_eq!(status.total_secs, 420);
        assert_eq!(status.until_unix_ms, now + 420 * 1000);

        assert_eq!(status.remaining_secs(now), 420);
        assert_eq!(status.remaining_secs(now + 10_000), 410);
        assert_eq!(status.remaining_secs(status.until_unix_ms), 0);
        assert_eq!(status.remaining_secs(status.until_unix_ms + 5_000), 0);
    }
}
