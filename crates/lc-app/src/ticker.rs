use crate::state::{AppMode, AppState};
use lc_core::geo::{destination, haversine_m};
use lc_core::LatLng;
use rand::Rng;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;

pub fn start_ticker(app_state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut last_tick = Instant::now();
        let mut stationary_anchor: Option<LatLng> = None;
        let mut last_drift_tick = Instant::now();

        loop {
            // Get interval from profile
            let interval_ms = {
                let lock = app_state.inner.read().await;
                lock.profile.update_interval_ms.max(100)
            };

            tokio::time::sleep(Duration::from_millis(interval_ms)).await;

            let now = Instant::now();
            let dt = now.duration_since(last_tick);
            last_tick = now;

            let mut should_broadcast = false;
            let mut pos_to_push: Option<LatLng> = None;

            {
                let mut state = app_state.inner.write().await;

                match state.mode {
                    AppMode::Route => {
                        if let Some(ref mut ar) = state.active_route {
                            if !ar.runner.is_paused() && !ar.runner.is_finished() {
                                let tick = ar.runner.step(dt);
                                state.position = Some(tick.position);
                                state.heading_deg = tick.heading_deg;
                                state.pushes += 1;
                                pos_to_push = Some(tick.position);
                                should_broadcast = true;
                                stationary_anchor = None;
                            }
                        }
                    }
                    AppMode::Joystick if state.joystick.active => {
                        let curr_pos = state.position.unwrap_or(LatLng::new(25.033964, 121.564468));
                        let step_m = (state.speed_kmh * 1000.0 / 3600.0) * dt.as_secs_f64();
                        let new_pos = destination(curr_pos, state.joystick.heading_deg, step_m);

                        state.position = Some(new_pos);
                        state.heading_deg = state.joystick.heading_deg;
                        state.pushes += 1;
                        pos_to_push = Some(new_pos);
                        should_broadcast = true;
                        stationary_anchor = None;
                    }
                    _ => {}
                }

                // Stationary natural micro-drift: when stationary, wander ~0.2m–0.8m around anchor
                if pos_to_push.is_none() {
                    if let Some(curr_pos) = state.position {
                        if state.profile.stationary_drift_m > 0.0 {
                            let anchor = match stationary_anchor {
                                Some(a) => {
                                    if haversine_m(a, curr_pos)
                                        > (state.profile.stationary_drift_m * 2.5).max(3.0)
                                    {
                                        stationary_anchor = Some(curr_pos);
                                        curr_pos
                                    } else {
                                        a
                                    }
                                }
                                None => {
                                    stationary_anchor = Some(curr_pos);
                                    curr_pos
                                }
                            };

                            if now.duration_since(last_drift_tick) >= Duration::from_millis(2500) {
                                last_drift_tick = now;
                                let mut rng = rand::thread_rng();
                                let min_drift = 0.2f64.min(state.profile.stationary_drift_m);
                                let dist = if min_drift < state.profile.stationary_drift_m {
                                    rng.gen_range(min_drift..=state.profile.stationary_drift_m)
                                } else {
                                    state.profile.stationary_drift_m
                                };
                                let bearing = rng.gen_range(0.0..360.0);
                                let drifted = destination(anchor, bearing, dist);

                                state.position = Some(drifted);
                                state.pushes += 1;
                                pos_to_push = Some(drifted);
                                should_broadcast = true;
                            }
                        }
                    }
                }

                // Check cooldown expiry
                if let Some(ref mut cd) = state.cooldown {
                    let now_ms = crate::state::InnerState::now_unix_ms();
                    if crate::cooldown_util::update_cooldown(cd, now_ms) {
                        should_broadcast = true;
                    }
                }
            }

            // Push to session outside the state.inner lock
            if let Some(target) = pos_to_push {
                let mut sess_lock = app_state.session.lock().await;
                if let Some(ref mut session) = *sess_lock {
                    if let Err(e) = session.set_location(target.lat, target.lng).await {
                        tracing::warn!("Failed to push location to device: {e:#}");
                        let mut state = app_state.inner.write().await;
                        state.last_error = Some(format!("Push location failed: {e:#}"));
                    }
                }
            }

            if should_broadcast {
                app_state.broadcast().await;
            }
        }
    })
}
