use crate::cooldown_util::{update_cooldown, CooldownView};
use crate::favorites::FavoritesStore;
use crate::profile_util::default_bloom;
use lc_core::engine::{MovementPlan, Mover, MoverTick, RouteMode};
use lc_core::geo::{destination, path_length_m, point_along};
use lc_core::profile::GameProfile;
use lc_core::LatLng;
use lc_device::{ConnectOptions, DeviceInfo, LocationSession};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex, RwLock};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppMode {
    #[default]
    Idle,
    Teleport,
    Route,
    Joystick,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RouteStateView {
    pub path: Vec<LatLng>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lead_in: Vec<LatLng>,
    pub lap_progress_m: f64,
    pub lap_length_m: f64,
    pub total_travelled_m: f64,
    pub lap: u32,
    pub repeat: Option<u32>,
    pub route_mode: String,
    pub paused: bool,
    pub finished: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppStateView {
    pub device: Option<DeviceInfo>,
    pub connected: bool,
    pub position: Option<LatLng>,
    pub heading_deg: f64,
    pub mode: AppMode,
    pub speed_kmh: f64,
    pub profile: GameProfile,
    pub route: Option<RouteStateView>,
    pub cooldown: Option<CooldownView>,
    pub pushes: u64,
    pub last_error: Option<String>,
    pub server_time_unix_ms: u64,
}

pub struct FallbackMover {
    plan: MovementPlan,
    lead_in_length_m: f64,
    lead_in_progress_m: f64,
    lead_in_done: bool,
    lap_length_m: f64,
    lap_progress_m: f64,
    total_travelled_m: f64,
    lap: u32,
    paused: bool,
    finished: bool,
    forward: bool,
    seed: u64,
}

impl FallbackMover {
    pub fn new(plan: MovementPlan, seed: u64) -> anyhow::Result<Self> {
        if plan.path.len() < 2 {
            anyhow::bail!("Path must contain at least 2 points");
        }
        for pt in &plan.path {
            if !pt.is_valid() {
                anyhow::bail!("Path contains invalid coordinates");
            }
        }
        for pt in &plan.lead_in {
            if !pt.is_valid() {
                anyhow::bail!("Lead-in contains invalid coordinates");
            }
        }
        if plan.speed_kmh <= 0.0 {
            anyhow::bail!("Speed must be positive");
        }

        let (lead_in_length_m, lead_in_done) = if plan.lead_in.len() >= 2 {
            (path_length_m(&plan.lead_in), false)
        } else {
            (0.0, true)
        };

        let lap_length_m = path_length_m(&plan.path);

        Ok(Self {
            plan,
            lead_in_length_m,
            lead_in_progress_m: 0.0,
            lead_in_done,
            lap_length_m,
            lap_progress_m: 0.0,
            total_travelled_m: 0.0,
            lap: 0,
            paused: false,
            finished: false,
            forward: true,
            seed,
        })
    }

    pub fn step(&mut self, dt: Duration) -> MoverTick {
        if self.paused || self.finished {
            return self.current();
        }

        let mut actual_speed_kmh = self.plan.speed_kmh;

        // Apply realistic variance if enabled
        if self.plan.realistic && self.plan.speed_variance > 0.0 {
            self.seed = self.seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let frac = ((self.seed >> 32) as f64) / (u32::MAX as f64);
            let var = (frac * 2.0 - 1.0) * self.plan.speed_variance;
            actual_speed_kmh = (self.plan.speed_kmh * (1.0 + var)).max(0.5);
        }

        let step_m = (actual_speed_kmh * 1000.0 / 3600.0) * dt.as_secs_f64();
        let mut remaining_step = step_m;

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

        if remaining_step > 0.0 {
            self.lap_progress_m += remaining_step;
            self.total_travelled_m += remaining_step;

            if self.lap_length_m <= 0.0 {
                self.finished = true;
                return self.current();
            }

            match self.plan.mode {
                RouteMode::Once => {
                    if self.lap_progress_m >= self.lap_length_m {
                        self.lap_progress_m = self.lap_length_m;
                        self.finished = true;
                    }
                }
                RouteMode::Loop => {
                    while self.lap_progress_m >= self.lap_length_m {
                        self.lap_progress_m -= self.lap_length_m;
                        self.lap += 1;
                        if let Some(repeat) = self.plan.repeat {
                            if self.lap >= repeat {
                                self.finished = true;
                                self.lap_progress_m = self.lap_length_m;
                                break;
                            }
                        }
                    }
                }
                RouteMode::RoundTrip => {
                    while self.lap_progress_m >= self.lap_length_m {
                        self.lap_progress_m -= self.lap_length_m;
                        self.forward = !self.forward;
                        if self.forward {
                            self.lap += 1;
                            if let Some(repeat) = self.plan.repeat {
                                if self.lap >= repeat {
                                    self.finished = true;
                                    self.lap_progress_m = self.lap_length_m;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut tick = self.current();
        tick.speed_kmh = actual_speed_kmh;
        tick
    }

    pub fn current(&self) -> MoverTick {
        let (on_path, brg, lap_progress_m, lap_length_m) = if !self.lead_in_done {
            let (pt, _, b) = point_along(&self.plan.lead_in, self.lead_in_progress_m);
            (pt, b, self.lead_in_progress_m, self.lead_in_length_m)
        } else {
            let (target_dist, is_fwd) = match self.plan.mode {
                RouteMode::RoundTrip => {
                    if self.forward {
                        (self.lap_progress_m, true)
                    } else {
                        (self.lap_length_m - self.lap_progress_m, false)
                    }
                }
                _ => (self.lap_progress_m, true),
            };

            let (pt, _, mut b) = point_along(&self.plan.path, target_dist);
            if !is_fwd {
                b = (b + 180.0) % 360.0;
            }
            (pt, b, self.lap_progress_m, self.lap_length_m)
        };

        let mut position = on_path;
        if self.plan.realistic && self.plan.jitter_m > 0.0 {
            let perp_brg = (brg + 90.0) % 360.0;
            let jitter_dist = ((self.seed & 0xFF) as f64 / 255.0 * 2.0 - 1.0) * self.plan.jitter_m;
            position = destination(on_path, perp_brg, jitter_dist);
        }

        MoverTick {
            position,
            on_path,
            heading_deg: brg,
            speed_kmh: self.plan.speed_kmh,
            lap_progress_m,
            lap_length_m,
            total_travelled_m: self.total_travelled_m,
            lap: self.lap,
            finished: self.finished,
        }
    }

    pub fn set_speed(&mut self, kmh: f64) {
        if kmh > 0.0 {
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

pub enum RouteRunner {
    Core(Box<Mover>),
    Fallback(FallbackMover),
}

impl RouteRunner {
    pub fn new(plan: MovementPlan, seed: u64) -> anyhow::Result<Self> {
        let plan_clone = plan.clone();
        let core_try = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Mover::new(plan_clone, seed)
        }));

        if let Ok(Ok(mover)) = core_try {
            return Ok(Self::Core(Box::new(mover)));
        }

        let fallback = FallbackMover::new(plan, seed)?;
        Ok(Self::Fallback(fallback))
    }

    pub fn step(&mut self, dt: Duration) -> MoverTick {
        match self {
            Self::Core(m) => {
                let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| m.step(dt)));
                match res {
                    Ok(tick) => tick,
                    Err(_) => m.current(),
                }
            }
            Self::Fallback(f) => f.step(dt),
        }
    }

    pub fn current(&self) -> MoverTick {
        match self {
            Self::Core(m) => m.current(),
            Self::Fallback(f) => f.current(),
        }
    }

    pub fn set_speed(&mut self, kmh: f64) {
        match self {
            Self::Core(m) => m.set_speed(kmh),
            Self::Fallback(f) => f.set_speed(kmh),
        }
    }

    pub fn pause(&mut self) {
        match self {
            Self::Core(m) => m.pause(),
            Self::Fallback(f) => f.pause(),
        }
    }

    pub fn resume(&mut self) {
        match self {
            Self::Core(m) => m.resume(),
            Self::Fallback(f) => f.resume(),
        }
    }

    pub fn is_paused(&self) -> bool {
        match self {
            Self::Core(m) => m.is_paused(),
            Self::Fallback(f) => f.is_paused(),
        }
    }

    pub fn is_finished(&self) -> bool {
        match self {
            Self::Core(m) => m.is_finished(),
            Self::Fallback(f) => f.is_finished(),
        }
    }
}

pub struct ActiveRoute {
    pub runner: RouteRunner,
    pub path: Vec<LatLng>,
    pub lead_in: Vec<LatLng>,
    pub route_mode: String,
    pub repeat: Option<u32>,
}

pub struct JoystickState {
    pub active: bool,
    pub heading_deg: f64,
}

pub struct InnerState {
    pub device: Option<DeviceInfo>,
    pub connected: bool,
    pub position: Option<LatLng>,
    pub heading_deg: f64,
    pub mode: AppMode,
    pub speed_kmh: f64,
    pub profile: GameProfile,
    pub active_route: Option<ActiveRoute>,
    pub joystick: JoystickState,
    pub cooldown: Option<CooldownView>,
    pub pushes: u64,
    pub last_error: Option<String>,
}

impl InnerState {
    pub fn now_unix_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub fn to_view(&self) -> AppStateView {
        let now = Self::now_unix_ms();
        let mut cooldown = self.cooldown.clone();
        if let Some(ref mut c) = cooldown {
            update_cooldown(c, now);
        }

        let route = self.active_route.as_ref().map(|ar| {
            let tick = ar.runner.current();
            RouteStateView {
                path: ar.path.clone(),
                lead_in: ar.lead_in.clone(),
                lap_progress_m: tick.lap_progress_m,
                lap_length_m: tick.lap_length_m,
                total_travelled_m: tick.total_travelled_m,
                lap: tick.lap,
                repeat: ar.repeat,
                route_mode: ar.route_mode.clone(),
                paused: ar.runner.is_paused(),
                finished: ar.runner.is_finished(),
            }
        });

        AppStateView {
            device: self.device.clone(),
            connected: self.connected,
            position: self.position,
            heading_deg: self.heading_deg,
            mode: self.mode,
            speed_kmh: self.speed_kmh,
            profile: self.profile.clone(),
            route,
            cooldown,
            pushes: self.pushes,
            last_error: self.last_error.clone(),
            server_time_unix_ms: now,
        }
    }
}

pub struct AppState {
    pub inner: RwLock<InnerState>,
    pub session: Mutex<Option<Box<dyn LocationSession>>>,
    pub tx: broadcast::Sender<AppStateView>,
    pub connect_opts: ConnectOptions,
    pub favorites: FavoritesStore,
    pub http_client: reqwest::Client,
}

impl AppState {
    pub fn new(connect_opts: ConnectOptions) -> Arc<Self> {
        let (tx, _) = broadcast::channel(128);
        let default_prof = default_bloom();
        let default_speed = default_prof.default_speed_kmh;

        let inner = InnerState {
            device: None,
            connected: false,
            position: None,
            heading_deg: 0.0,
            mode: AppMode::Idle,
            speed_kmh: default_speed,
            profile: default_prof,
            active_route: None,
            joystick: JoystickState {
                active: false,
                heading_deg: 0.0,
            },
            cooldown: None,
            pushes: 0,
            last_error: None,
        };

        Arc::new(Self {
            inner: RwLock::new(inner),
            session: Mutex::new(None),
            tx,
            connect_opts,
            favorites: FavoritesStore::new(),
            http_client: reqwest::Client::new(),
        })
    }

    pub async fn broadcast(&self) {
        let view = {
            let lock = self.inner.read().await;
            lock.to_view()
        };
        let _ = self.tx.send(view);
    }
}
