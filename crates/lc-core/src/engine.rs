//! Deterministic, time-step driven movement engine. No I/O, no clocks: the caller
//! passes elapsed time to `step`, which makes everything unit-testable.
use crate::LatLng;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RouteMode {
    /// Walk the path once and stop at the end.
    #[default]
    Once,
    /// Circle route: after the last point walk back to the first and repeat.
    Loop,
    /// Back and forth: A→B→A→B ...
    RoundTrip,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MovementPlan {
    pub path: Vec<LatLng>,
    /// Optional one-off approach path traversed once before beginning the main route/laps.
    /// Ideal for walking smoothly from current location (e.g. center) to the loop perimeter.
    #[serde(default)]
    pub lead_in: Vec<LatLng>,
    pub speed_kmh: f64,
    #[serde(default)]
    pub mode: RouteMode,
    /// Number of laps for Loop/RoundTrip (one lap of RoundTrip = there and back). None = infinite.
    #[serde(default)]
    pub repeat: Option<u32>,
    /// Realistic mode: speed varies by ±`speed_variance`, plus lateral jitter.
    #[serde(default)]
    pub realistic: bool,
    #[serde(default)]
    pub speed_variance: f64,
    #[serde(default)]
    pub jitter_m: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct MoverTick {
    /// Position to push to the device (includes jitter).
    pub position: LatLng,
    /// Position on the ideal path (no jitter) – used for drawing.
    pub on_path: LatLng,
    pub heading_deg: f64,
    /// Instantaneous speed used for this step.
    pub speed_kmh: f64,
    /// Distance travelled within the current lap (or lead-in), metres.
    pub lap_progress_m: f64,
    /// Length of one lap (or lead-in), metres.
    pub lap_length_m: f64,
    /// Total distance travelled since start, metres.
    pub total_travelled_m: f64,
    /// 0-based current lap.
    pub lap: u32,
    pub finished: bool,
}

/// Follows a `MovementPlan`.
pub struct Mover {
    plan: MovementPlan,
    lead_in_path: Vec<LatLng>,
    lead_in_cum_lengths: Vec<f64>,
    lead_in_length_m: f64,
    lead_in_progress_m: f64,
    lead_in_done: bool,

    lap_path: Vec<LatLng>,
    cum_lengths: Vec<f64>,
    lap_length_m: f64,
    lap_progress_m: f64,
    total_travelled_m: f64,
    lap: u32,
    speed_kmh: f64,
    speed_offset: f64,
    current_jitter: f64,
    paused: bool,
    finished: bool,
    rng: rand::rngs::StdRng,
}

impl Mover {
    /// Errors if the path has < 2 points, contains invalid coords, or speed <= 0.
    pub fn new(mut plan: MovementPlan, seed: u64) -> anyhow::Result<Self> {
        use rand::SeedableRng;

        if plan.path.len() < 2 {
            anyhow::bail!("path must contain at least 2 points");
        }
        for (i, p) in plan.path.iter().enumerate() {
            if !p.is_valid() {
                anyhow::bail!("invalid coordinate at index {i}: {p:?}");
            }
        }
        if !plan.speed_kmh.is_finite() || plan.speed_kmh <= 0.0 {
            anyhow::bail!("speed must be positive and finite, got {}", plan.speed_kmh);
        }

        plan.speed_variance = plan.speed_variance.max(0.0);
        plan.jitter_m = plan.jitter_m.max(0.0);

        let (lead_in_path, lead_in_cum_lengths, lead_in_length_m, lead_in_done) =
            if plan.lead_in.len() >= 2 {
                for (i, p) in plan.lead_in.iter().enumerate() {
                    if !p.is_valid() {
                        anyhow::bail!("invalid coordinate in lead_in at index {i}: {p:?}");
                    }
                }
                let cum = crate::geo::cumulative_m(&plan.lead_in);
                let total = *cum.last().unwrap_or(&0.0);
                let done = total <= 0.0;
                (plan.lead_in.clone(), cum, total, done)
            } else {
                (Vec::new(), Vec::new(), 0.0, true)
            };

        let lap_path = match plan.mode {
            RouteMode::Once => plan.path.clone(),
            RouteMode::Loop => {
                let mut p = plan.path.clone();
                if p.first() != p.last() {
                    p.push(p[0]);
                }
                p
            }
            RouteMode::RoundTrip => {
                let mut p = plan.path.clone();
                for &pt in plan.path.iter().rev().skip(1) {
                    p.push(pt);
                }
                p
            }
        };

        let cum_lengths = crate::geo::cumulative_m(&lap_path);
        let lap_length_m = *cum_lengths.last().unwrap_or(&0.0);

        let finished = (lap_length_m <= 0.0 && lead_in_done)
            || match plan.mode {
                RouteMode::Once => false,
                RouteMode::Loop | RouteMode::RoundTrip => plan.repeat == Some(0),
            };

        let speed_kmh = plan.speed_kmh;
        let rng = rand::rngs::StdRng::seed_from_u64(seed);

        Ok(Self {
            plan,
            lead_in_path,
            lead_in_cum_lengths,
            lead_in_length_m,
            lead_in_progress_m: 0.0,
            lead_in_done,
            lap_path,
            cum_lengths,
            lap_length_m,
            lap_progress_m: 0.0,
            total_travelled_m: 0.0,
            lap: 0,
            speed_kmh,
            speed_offset: 0.0,
            current_jitter: 0.0,
            paused: false,
            finished,
            rng,
        })
    }

    pub fn plan(&self) -> &MovementPlan {
        &self.plan
    }

    fn compute_tick(&self, speed: f64) -> MoverTick {
        let (on_path, heading_deg, lap_progress_m, lap_length_m) = if !self.lead_in_done {
            let (pt, _, h) = crate::geo::locate(
                &self.lead_in_path,
                &self.lead_in_cum_lengths,
                self.lead_in_progress_m,
            );
            (pt, h, self.lead_in_progress_m, self.lead_in_length_m)
        } else {
            let (pt, _, h) = crate::geo::locate(
                &self.lap_path,
                &self.cum_lengths,
                self.lap_progress_m,
            );
            (pt, h, self.lap_progress_m, self.lap_length_m)
        };

        let position = if self.plan.realistic
            && self.plan.jitter_m > 0.0
            && self.current_jitter.abs() > 1e-9
        {
            let jitter = self.current_jitter;
            let brg = if jitter >= 0.0 {
                crate::geo::normalize_bearing(heading_deg + 90.0)
            } else {
                crate::geo::normalize_bearing(heading_deg - 90.0)
            };
            crate::geo::destination(on_path, brg, jitter.abs())
        } else {
            on_path
        };

        MoverTick {
            position,
            on_path,
            heading_deg,
            speed_kmh: speed,
            lap_progress_m,
            lap_length_m,
            total_travelled_m: self.total_travelled_m,
            lap: self.lap,
            finished: self.finished,
        }
    }

    /// True if the mover is still in its approach/lead-in segment.
    pub fn is_in_lead_in(&self) -> bool {
        !self.lead_in_done
    }

    /// Advance by `dt` (ignored while paused) and return the new state.
    pub fn step(&mut self, dt: Duration) -> MoverTick {
        if self.paused || self.finished || dt.is_zero() {
            return self.current();
        }

        if self.plan.realistic {
            use rand::Rng;
            if self.plan.speed_variance > 0.0 {
                let max_v = self.plan.speed_variance;
                let r: f64 = self.rng.gen_range(-1.0..=1.0);
                let delta = r * (max_v * 0.1) - self.speed_offset * 0.05;
                self.speed_offset = (self.speed_offset + delta).clamp(-max_v, max_v);
            }
            if self.plan.jitter_m > 0.0 {
                let max_j = self.plan.jitter_m;
                let r: f64 = self.rng.gen_range(-1.0..=1.0);
                let delta = r * (max_j * 0.1) - self.current_jitter * 0.05;
                self.current_jitter = (self.current_jitter + delta).clamp(-max_j, max_j);
            }
        }

        let current_speed = if self.plan.realistic && self.plan.speed_variance > 0.0 {
            (self.speed_kmh * (1.0 + self.speed_offset)).max(0.01)
        } else {
            self.speed_kmh
        };

        let speed_ms = current_speed * 1000.0 / 3600.0;
        let step_dist_m = speed_ms * dt.as_secs_f64();

        let mut remaining_step = step_dist_m;

        // Traverse lead-in if not yet completed
        if !self.lead_in_done {
            let dist_to_lead_end = self.lead_in_length_m - self.lead_in_progress_m;
            if remaining_step < dist_to_lead_end {
                self.lead_in_progress_m += remaining_step;
                self.total_travelled_m += remaining_step;
                remaining_step = 0.0;
            } else {
                self.total_travelled_m += dist_to_lead_end;
                self.lead_in_progress_m = self.lead_in_length_m;
                remaining_step -= dist_to_lead_end;
                self.lead_in_done = true;
            }
        }

        while remaining_step > 0.0 && !self.finished && self.lap_length_m > 0.0 {
            let dist_to_lap_end = self.lap_length_m - self.lap_progress_m;
            if remaining_step < dist_to_lap_end {
                self.lap_progress_m += remaining_step;
                self.total_travelled_m += remaining_step;
                remaining_step = 0.0;
            } else {
                self.total_travelled_m += dist_to_lap_end;
                self.lap_progress_m = self.lap_length_m;
                remaining_step -= dist_to_lap_end;

                let max_laps = match self.plan.mode {
                    RouteMode::Once => Some(1),
                    RouteMode::Loop | RouteMode::RoundTrip => self.plan.repeat,
                };

                if let Some(limit) = max_laps {
                    if self.lap + 1 >= limit {
                        self.finished = true;
                        break;
                    }
                }

                self.lap += 1;
                self.lap_progress_m = 0.0;
            }
        }

        self.compute_tick(current_speed)
    }

    /// Current state without advancing.
    pub fn current(&self) -> MoverTick {
        let speed = if self.plan.realistic && self.plan.speed_variance > 0.0 {
            (self.speed_kmh * (1.0 + self.speed_offset)).max(0.01)
        } else {
            self.speed_kmh
        };
        self.compute_tick(speed)
    }

    pub fn set_speed(&mut self, kmh: f64) {
        if kmh.is_finite() && kmh > 0.0 {
            self.speed_kmh = kmh;
            self.plan.speed_kmh = kmh;
        }
    }

    pub fn pause(&mut self) {
        self.paused = true;
    }

    pub fn resume(&mut self) {
        self.paused = false;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }
}

/// Free movement controlled by a direction (joystick / WASD).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Joystick {
    pub position: LatLng,
    pub heading_deg: f64,
    pub speed_kmh: f64,
    /// When false, `step` does not move.
    pub active: bool,
}

impl Joystick {
    pub fn new(position: LatLng, speed_kmh: f64) -> Self {
        Self {
            position,
            heading_deg: 0.0,
            speed_kmh,
            active: false,
        }
    }

    /// Move along heading for `dt` if active; returns the new position.
    pub fn step(&mut self, dt: Duration) -> LatLng {
        if !self.active || dt.is_zero() || !self.speed_kmh.is_finite() || self.speed_kmh <= 0.0 {
            return self.position;
        }
        let speed_ms = self.speed_kmh * 1000.0 / 3600.0;
        let dist_m = speed_ms * dt.as_secs_f64();
        self.position = crate::geo::destination(self.position, self.heading_deg, dist_m);
        self.position
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PT_A: LatLng = LatLng::new(25.033964, 121.564468);
    const PT_B: LatLng = LatLng::new(25.035000, 121.564468);
    const PT_C: LatLng = LatLng::new(25.035000, 121.566000);

    #[test]
    fn test_mover_validation() {
        let bad_path = MovementPlan {
            path: vec![PT_A],
            lead_in: Vec::new(),
            speed_kmh: 5.0,
            mode: RouteMode::Once,
            repeat: None,
            realistic: false,
            speed_variance: 0.0,
            jitter_m: 0.0,
        };
        assert!(Mover::new(bad_path, 42).is_err());

        let invalid_coords = MovementPlan {
            path: vec![PT_A, LatLng::new(100.0, 0.0)],
            lead_in: Vec::new(),
            speed_kmh: 5.0,
            mode: RouteMode::Once,
            repeat: None,
            realistic: false,
            speed_variance: 0.0,
            jitter_m: 0.0,
        };
        assert!(Mover::new(invalid_coords, 42).is_err());

        let zero_speed = MovementPlan {
            path: vec![PT_A, PT_B],
            lead_in: Vec::new(),
            speed_kmh: 0.0,
            mode: RouteMode::Once,
            repeat: None,
            realistic: false,
            speed_variance: 0.0,
            jitter_m: 0.0,
        };
        assert!(Mover::new(zero_speed, 42).is_err());
    }

    #[test]
    fn test_mover_once_mode() {
        let plan = MovementPlan {
            path: vec![PT_A, PT_B],
            lead_in: Vec::new(),
            speed_kmh: 3.6, // 1 m/s
            mode: RouteMode::Once,
            repeat: None,
            realistic: false,
            speed_variance: 0.0,
            jitter_m: 0.0,
        };
        let mut mover = Mover::new(plan, 12345).expect("create mover");
        let path_len = crate::geo::haversine_m(PT_A, PT_B);
        assert!((mover.lap_length_m - path_len).abs() < 1e-6);

        let initial = mover.current();
        assert_eq!(initial.position, PT_A);
        assert_eq!(initial.lap, 0);
        assert_eq!(initial.lap_progress_m, 0.0);
        assert!(!initial.finished);

        // Step 10 seconds -> 10 meters
        let tick = mover.step(Duration::from_secs(10));
        assert!((tick.lap_progress_m - 10.0).abs() < 1e-4);
        assert!((tick.total_travelled_m - 10.0).abs() < 1e-4);
        assert!(!tick.finished);

        // Step past the end
        let finish_step_secs = (path_len + 10.0) as u64;
        let final_tick = mover.step(Duration::from_secs(finish_step_secs));
        assert!(final_tick.finished);
        assert!(mover.is_finished());
        assert!((final_tick.lap_progress_m - path_len).abs() < 1e-6);
        assert!((final_tick.total_travelled_m - path_len).abs() < 1e-6);
        assert_eq!(final_tick.position, PT_B);

        // Subsequent steps should remain clamped and finished
        let after_tick = mover.step(Duration::from_secs(10));
        assert!(after_tick.finished);
        assert_eq!(after_tick.position, PT_B);
        assert_eq!(after_tick.total_travelled_m, final_tick.total_travelled_m);
    }

    #[test]
    fn test_mover_loop_mode_with_repeat() {
        let plan = MovementPlan {
            path: vec![PT_A, PT_B, PT_C],
            lead_in: Vec::new(),
            speed_kmh: 3.6, // 1 m/s
            mode: RouteMode::Loop,
            repeat: Some(2), // 2 laps
            realistic: false,
            speed_variance: 0.0,
            jitter_m: 0.0,
        };
        let mut mover = Mover::new(plan, 1).expect("loop mover");
        let expected_lap_len = crate::geo::haversine_m(PT_A, PT_B)
            + crate::geo::haversine_m(PT_B, PT_C)
            + crate::geo::haversine_m(PT_C, PT_A);
        assert!((mover.lap_length_m - expected_lap_len).abs() < 1e-4);

        // Step 1 full lap + 10m
        let step_secs = expected_lap_len + 10.0;
        let tick = mover.step(Duration::from_secs_f64(step_secs));
        assert_eq!(tick.lap, 1);
        assert!((tick.lap_progress_m - 10.0).abs() < 1e-3);
        assert!(!tick.finished);

        // Step enough to complete second lap
        mover.step(Duration::from_secs_f64(expected_lap_len));
        assert!(mover.is_finished());
        let current = mover.current();
        assert_eq!(current.lap, 1);
        assert!((current.lap_progress_m - expected_lap_len).abs() < 1e-3);
        assert!((current.total_travelled_m - 2.0 * expected_lap_len).abs() < 1e-3);
    }

    #[test]
    fn test_mover_roundtrip_mode() {
        let plan = MovementPlan {
            path: vec![PT_A, PT_B, PT_C],
            lead_in: Vec::new(),
            speed_kmh: 3.6,
            mode: RouteMode::RoundTrip,
            repeat: Some(1),
            realistic: false,
            speed_variance: 0.0,
            jitter_m: 0.0,
        };
        let mut mover = Mover::new(plan, 1).expect("roundtrip mover");
        let fwd_len = crate::geo::haversine_m(PT_A, PT_B) + crate::geo::haversine_m(PT_B, PT_C);
        let expected_lap_len = 2.0 * fwd_len;
        assert!((mover.lap_length_m - expected_lap_len).abs() < 1e-4);

        // Complete the lap
        mover.step(Duration::from_secs_f64(expected_lap_len + 5.0));
        assert!(mover.is_finished());
        let tick = mover.current();
        assert_eq!(tick.position, PT_A);
        assert!((tick.total_travelled_m - expected_lap_len).abs() < 1e-4);
    }

    #[test]
    fn test_mover_pause_resume() {
        let plan = MovementPlan {
            path: vec![PT_A, PT_B],
            lead_in: Vec::new(),
            speed_kmh: 3.6,
            mode: RouteMode::Once,
            repeat: None,
            realistic: false,
            speed_variance: 0.0,
            jitter_m: 0.0,
        };
        let mut mover = Mover::new(plan, 1).unwrap();
        mover.pause();
        assert!(mover.is_paused());

        let tick = mover.step(Duration::from_secs(10));
        assert_eq!(tick.lap_progress_m, 0.0);

        mover.resume();
        assert!(!mover.is_paused());
        let tick = mover.step(Duration::from_secs(10));
        assert!((tick.lap_progress_m - 10.0).abs() < 1e-4);
    }

    #[test]
    fn test_mover_set_speed() {
        let plan = MovementPlan {
            path: vec![PT_A, PT_B],
            lead_in: Vec::new(),
            speed_kmh: 3.6, // 1 m/s
            mode: RouteMode::Once,
            repeat: None,
            realistic: false,
            speed_variance: 0.0,
            jitter_m: 0.0,
        };
        let mut mover = Mover::new(plan, 1).unwrap();
        mover.set_speed(7.2); // 2 m/s
        assert_eq!(mover.plan().speed_kmh, 7.2);

        let tick = mover.step(Duration::from_secs(10));
        assert!((tick.lap_progress_m - 20.0).abs() < 1e-4);
    }

    #[test]
    fn test_mover_realistic_jitter_and_speed_bounds() {
        let plan = MovementPlan {
            path: vec![PT_A, PT_B, PT_C],
            lead_in: Vec::new(),
            speed_kmh: 10.0,
            mode: RouteMode::Loop,
            repeat: None,
            realistic: true,
            speed_variance: 0.15,
            jitter_m: 2.0,
        };
        let mut mover = Mover::new(plan, 999).unwrap();

        for _ in 0..50 {
            let tick = mover.step(Duration::from_secs(1));
            // Check jitter bounds
            let jitter_dist = crate::geo::haversine_m(tick.position, tick.on_path);
            assert!(
                jitter_dist <= 2.0 + 1e-4,
                "jitter dist {jitter_dist} exceeded 2.0"
            );

            // Check speed variance bounds
            let min_speed = 10.0 * (1.0 - 0.15) - 1e-4;
            let max_speed = 10.0 * (1.0 + 0.15) + 1e-4;
            assert!(
                tick.speed_kmh >= min_speed && tick.speed_kmh <= max_speed,
                "speed {} out of bounds [{}, {}]",
                tick.speed_kmh,
                min_speed,
                max_speed
            );
        }
    }

    #[test]
    fn test_mover_lead_in_mode() {
        // PT_A is center, PT_B and PT_C form the loop
        let lead_in_dist = crate::geo::haversine_m(PT_A, PT_B);
        let loop_dist = crate::geo::haversine_m(PT_B, PT_C) + crate::geo::haversine_m(PT_C, PT_B);

        let plan = MovementPlan {
            path: vec![PT_B, PT_C],
            lead_in: vec![PT_A, PT_B],
            speed_kmh: 3.6, // 1 m/s
            mode: RouteMode::Loop,
            repeat: Some(2),
            realistic: false,
            speed_variance: 0.0,
            jitter_m: 0.0,
        };

        let mut mover = Mover::new(plan, 42).expect("create mover with lead_in");
        assert!(mover.is_in_lead_in());

        // Initial position is strictly PT_A (center) - ZERO teleportation!
        let initial = mover.current();
        assert_eq!(initial.position, PT_A);
        assert_eq!(initial.lap, 0);

        // Step halfway through lead-in
        let half_secs = lead_in_dist / 2.0;
        let tick = mover.step(Duration::from_secs_f64(half_secs));
        assert!(mover.is_in_lead_in());
        assert!((tick.lap_progress_m - half_secs).abs() < 1e-3);

        // Step through the rest of lead-in plus 5m into the loop
        let rest_secs = (lead_in_dist - half_secs) + 5.0;
        let tick = mover.step(Duration::from_secs_f64(rest_secs));
        // Should now be on the loop!
        assert!(!mover.is_in_lead_in());
        assert!((tick.lap_progress_m - 5.0).abs() < 1e-3);

        // Complete 2 laps on the loop
        let remaining_to_finish = (loop_dist - 5.0) + loop_dist;
        let final_tick = mover.step(Duration::from_secs_f64(remaining_to_finish + 1.0));
        assert!(mover.is_finished());
        assert_eq!(final_tick.lap, 1);
        // Total travelled must equal lead_in + 2 * loop_dist
        let expected_total = lead_in_dist + 2.0 * loop_dist;
        assert!((final_tick.total_travelled_m - expected_total).abs() < 1e-2);
    }

    #[test]
    fn test_joystick_step() {
        let mut js = Joystick::new(PT_A, 3.6); // 1 m/s
        assert_eq!(js.position, PT_A);
        assert!(!js.active);

        // Inactive step
        let pos = js.step(Duration::from_secs(10));
        assert_eq!(pos, PT_A);

        // Active step heading North (0 deg)
        js.active = true;
        js.heading_deg = 0.0;
        let pos = js.step(Duration::from_secs(10));
        let dist = crate::geo::haversine_m(PT_A, pos);
        assert!((dist - 10.0).abs() < 1e-3);
        let brg = crate::geo::bearing_deg(PT_A, pos);
        assert!(!(0.1..=359.9).contains(&brg));
    }
}
