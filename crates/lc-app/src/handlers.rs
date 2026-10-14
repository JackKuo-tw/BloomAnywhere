use crate::cooldown_util::create_cooldown;
use crate::favorites::Favorite;
use crate::gpx_util::{parse_gpx_content, to_gpx_content};
use crate::nominatim::{search_location, SearchResult};
use crate::osrm::{plan_route, RoutePlanRequest, RoutePlanResponse};
use crate::profile_util::{load_profile_by_id, load_profiles};
use crate::state::{ActiveRoute, AppMode, AppState, AppStateView, RouteRunner};
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use lc_core::engine::{MovementPlan, RouteMode};
use lc_core::geo::haversine_m;
use lc_core::profile::GameProfile;
use lc_core::LatLng;
use lc_device::DeviceInfo;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

pub struct AppError(pub StatusCode, pub String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl<E: std::fmt::Display> From<E> for AppError {
    fn from(err: E) -> Self {
        AppError(StatusCode::INTERNAL_SERVER_ERROR, err.to_string())
    }
}

// GET /api/devices
pub async fn list_devices(
    State(app_state): State<Arc<AppState>>,
) -> Result<Json<Vec<DeviceInfo>>, AppError> {
    let devices = lc_device::list_devices(&app_state.connect_opts)
        .await
        .map_err(|e| {
            AppError(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to list devices: {e:#}"),
            )
        })?;
    Ok(Json(devices))
}

#[derive(Deserialize)]
pub struct ConnectReq {
    pub id: String,
}

// POST /api/connect
pub async fn connect_device(
    State(app_state): State<Arc<AppState>>,
    Json(body): Json<ConnectReq>,
) -> Result<Json<AppStateView>, AppError> {
    let session = lc_device::connect(&body.id, &app_state.connect_opts)
        .await
        .map_err(|e| AppError(StatusCode::NOT_FOUND, format!("Connect failed: {e:#}")))?;

    let info = session.info().clone();

    {
        let mut sess_lock = app_state.session.lock().await;
        *sess_lock = Some(session);
    }

    {
        let mut state = app_state.inner.write().await;
        state.device = Some(info);
        state.connected = true;
        state.last_error = None;
    }

    app_state.broadcast().await;

    let view = {
        let lock = app_state.inner.read().await;
        lock.to_view()
    };
    Ok(Json(view))
}

// POST /api/disconnect
pub async fn disconnect_device(
    State(app_state): State<Arc<AppState>>,
) -> Result<Json<AppStateView>, AppError> {
    {
        let mut sess_lock = app_state.session.lock().await;
        if let Some(mut session) = sess_lock.take() {
            let _ = session.clear().await;
        }
    }

    {
        let mut state = app_state.inner.write().await;
        state.device = None;
        state.connected = false;
        state.active_route = None;
        state.joystick.active = false;
        state.mode = AppMode::Idle;
        state.last_error = None;
    }

    app_state.broadcast().await;

    let view = {
        let lock = app_state.inner.read().await;
        lock.to_view()
    };
    Ok(Json(view))
}

// GET /api/state
pub async fn get_state(State(app_state): State<Arc<AppState>>) -> Json<AppStateView> {
    let lock = app_state.inner.read().await;
    Json(lock.to_view())
}

// GET /api/profiles
pub async fn list_profiles() -> Json<Vec<GameProfile>> {
    Json(load_profiles())
}

#[derive(Deserialize)]
pub struct ProfileReq {
    pub id: String,
}

// POST /api/profile
pub async fn set_profile(
    State(app_state): State<Arc<AppState>>,
    Json(body): Json<ProfileReq>,
) -> Result<Json<AppStateView>, AppError> {
    let prof = load_profile_by_id(&body.id).ok_or_else(|| {
        AppError(
            StatusCode::NOT_FOUND,
            format!("Profile {} not found", body.id),
        )
    })?;

    {
        let mut state = app_state.inner.write().await;
        state.speed_kmh = state.speed_kmh.min(prof.max_speed_kmh);
        state.profile = prof;
    }

    app_state.broadcast().await;

    let view = {
        let lock = app_state.inner.read().await;
        lock.to_view()
    };
    Ok(Json(view))
}

#[derive(Deserialize)]
pub struct TeleportReq {
    pub lat: f64,
    pub lng: f64,
}

// POST /api/teleport
pub async fn teleport(
    State(app_state): State<Arc<AppState>>,
    Json(body): Json<TeleportReq>,
) -> Result<Json<AppStateView>, AppError> {
    let target = LatLng::new(body.lat, body.lng);
    if !target.is_valid() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Invalid coordinates".into(),
        ));
    }

    let now_ms = crate::state::InnerState::now_unix_ms();

    {
        let mut sess_lock = app_state.session.lock().await;
        if let Some(ref mut session) = *sess_lock {
            session
                .set_location(target.lat, target.lng)
                .await
                .map_err(|e| {
                    AppError(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to set location: {e:#}"),
                    )
                })?;
        }
    }

    {
        let mut state = app_state.inner.write().await;

        // Check distance for cooldown
        if let Some(prev) = state.position {
            let dist_m = haversine_m(prev, target);
            let dist_km = dist_m / 1000.0;
            if dist_km >= 1.0 && state.profile.cooldown_enabled {
                state.cooldown = create_cooldown(dist_km, now_ms);
            }
        }

        state.position = Some(target);
        state.mode = AppMode::Teleport;
        state.pushes += 1;
        state.last_error = None;
    }

    app_state.broadcast().await;

    let view = {
        let lock = app_state.inner.read().await;
        lock.to_view()
    };
    Ok(Json(view))
}

// POST /api/route/plan
pub async fn route_plan(
    State(app_state): State<Arc<AppState>>,
    Json(body): Json<RoutePlanRequest>,
) -> Result<Json<RoutePlanResponse>, AppError> {
    let resp = plan_route(&app_state.http_client, &body.points, &body.profile).await;
    Ok(Json(resp))
}

#[derive(Deserialize)]
pub struct RouteStartReq {
    pub points: Vec<LatLng>,
    #[serde(default)]
    pub lead_in: Vec<LatLng>,
    pub speed_kmh: f64,
    #[serde(default = "default_mode_str")]
    pub mode: String, // "once", "loop", "round_trip"
    pub repeat: Option<u32>,
    #[serde(default)]
    pub realistic: bool,
    #[serde(default = "default_snap_str")]
    pub snap: String, // "foot", "bike", "car", "straight"
}

fn default_mode_str() -> String {
    "once".into()
}
fn default_snap_str() -> String {
    "foot".into()
}

// POST /api/route/start
pub async fn route_start(
    State(app_state): State<Arc<AppState>>,
    Json(body): Json<RouteStartReq>,
) -> Result<Json<AppStateView>, AppError> {
    if body.points.len() < 2 {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Path requires at least 2 points".into(),
        ));
    }
    if body.speed_kmh <= 0.0 {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Speed must be positive".into(),
        ));
    }

    // Plan route if snapped
    let planned = plan_route(&app_state.http_client, &body.points, &body.snap).await;
    let path = planned.path;

    // Plan lead-in approach if provided
    let planned_lead_in = if body.lead_in.len() >= 2 {
        let lead_planned = plan_route(&app_state.http_client, &body.lead_in, &body.snap).await;
        lead_planned.path
    } else {
        Vec::new()
    };

    let route_mode = match body.mode.as_str() {
        "loop" => RouteMode::Loop,
        "round_trip" => RouteMode::RoundTrip,
        _ => RouteMode::Once,
    };

    let (profile_jitter, profile_var) = {
        let lock = app_state.inner.read().await;
        (lock.profile.jitter_m, lock.profile.speed_variance)
    };

    let plan = MovementPlan {
        path: path.clone(),
        lead_in: planned_lead_in.clone(),
        speed_kmh: body.speed_kmh,
        mode: route_mode,
        repeat: body.repeat,
        realistic: body.realistic,
        speed_variance: if body.realistic { profile_var } else { 0.0 },
        jitter_m: if body.realistic { profile_jitter } else { 0.0 },
    };

    let seed = rand::random::<u64>();
    let runner = RouteRunner::new(plan, seed).map_err(|e| {
        AppError(
            StatusCode::BAD_REQUEST,
            format!("Route creation failed: {e:#}"),
        )
    })?;

    let first_pos = if !planned_lead_in.is_empty() {
        planned_lead_in[0]
    } else {
        path[0]
    };

    {
        let mut sess_lock = app_state.session.lock().await;
        if let Some(ref mut session) = *sess_lock {
            let _ = session.set_location(first_pos.lat, first_pos.lng).await;
        }
    }

    {
        let mut state = app_state.inner.write().await;
        state.position = Some(first_pos);
        state.mode = AppMode::Route;
        state.speed_kmh = body.speed_kmh;
        state.active_route = Some(ActiveRoute {
            runner,
            path,
            lead_in: planned_lead_in,
            route_mode: body.mode,
            repeat: body.repeat,
        });
        state.pushes += 1;
        state.last_error = None;
    }

    app_state.broadcast().await;

    let view = {
        let lock = app_state.inner.read().await;
        lock.to_view()
    };
    Ok(Json(view))
}

// POST /api/route/pause
pub async fn route_pause(
    State(app_state): State<Arc<AppState>>,
) -> Result<Json<AppStateView>, AppError> {
    {
        let mut state = app_state.inner.write().await;
        if let Some(ref mut ar) = state.active_route {
            ar.runner.pause();
        }
    }

    app_state.broadcast().await;

    let view = {
        let lock = app_state.inner.read().await;
        lock.to_view()
    };
    Ok(Json(view))
}

// POST /api/route/resume
pub async fn route_resume(
    State(app_state): State<Arc<AppState>>,
) -> Result<Json<AppStateView>, AppError> {
    {
        let mut state = app_state.inner.write().await;
        if let Some(ref mut ar) = state.active_route {
            ar.runner.resume();
        }
    }

    app_state.broadcast().await;

    let view = {
        let lock = app_state.inner.read().await;
        lock.to_view()
    };
    Ok(Json(view))
}

// POST /api/route/stop
pub async fn route_stop(
    State(app_state): State<Arc<AppState>>,
) -> Result<Json<AppStateView>, AppError> {
    {
        let mut state = app_state.inner.write().await;
        state.active_route = None;
        if state.mode == AppMode::Route {
            state.mode = AppMode::Idle;
        }
    }

    app_state.broadcast().await;

    let view = {
        let lock = app_state.inner.read().await;
        lock.to_view()
    };
    Ok(Json(view))
}

#[derive(Deserialize)]
pub struct SpeedReq {
    pub speed_kmh: f64,
}

// POST /api/speed
pub async fn set_speed(
    State(app_state): State<Arc<AppState>>,
    Json(body): Json<SpeedReq>,
) -> Result<Json<AppStateView>, AppError> {
    if body.speed_kmh <= 0.0 {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Speed must be positive".into(),
        ));
    }

    {
        let mut state = app_state.inner.write().await;
        let clamped = body.speed_kmh.min(state.profile.max_speed_kmh);
        state.speed_kmh = clamped;
        if let Some(ref mut ar) = state.active_route {
            ar.runner.set_speed(clamped);
        }
    }

    app_state.broadcast().await;

    let view = {
        let lock = app_state.inner.read().await;
        lock.to_view()
    };
    Ok(Json(view))
}

#[derive(Deserialize)]
pub struct JoystickReq {
    pub active: bool,
    pub heading_deg: f64,
}

// POST /api/joystick
pub async fn set_joystick(
    State(app_state): State<Arc<AppState>>,
    Json(body): Json<JoystickReq>,
) -> Result<Json<AppStateView>, AppError> {
    {
        let mut state = app_state.inner.write().await;
        state.joystick.active = body.active;
        state.joystick.heading_deg = (body.heading_deg % 360.0 + 360.0) % 360.0;
        if body.active {
            state.mode = AppMode::Joystick;
            state.heading_deg = state.joystick.heading_deg;
        } else if state.mode == AppMode::Joystick {
            state.mode = AppMode::Idle;
        }
    }

    app_state.broadcast().await;

    let view = {
        let lock = app_state.inner.read().await;
        lock.to_view()
    };
    Ok(Json(view))
}

// POST /api/clear
pub async fn clear_location(
    State(app_state): State<Arc<AppState>>,
) -> Result<Json<AppStateView>, AppError> {
    {
        let mut sess_lock = app_state.session.lock().await;
        if let Some(ref mut session) = *sess_lock {
            session.clear().await.map_err(|e| {
                AppError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Clear failed: {e:#}"),
                )
            })?;
        }
    }

    {
        let mut state = app_state.inner.write().await;
        state.position = None;
        state.active_route = None;
        state.joystick.active = false;
        state.cooldown = None;
        state.mode = AppMode::Idle;
        state.last_error = None;
    }

    app_state.broadcast().await;

    let view = {
        let lock = app_state.inner.read().await;
        lock.to_view()
    };
    Ok(Json(view))
}

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: String,
}

// GET /api/search
pub async fn search(
    State(app_state): State<Arc<AppState>>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<SearchResult>>, AppError> {
    let results = search_location(&app_state.http_client, &query.q)
        .await
        .map_err(|e| AppError(StatusCode::BAD_GATEWAY, format!("Search failed: {e:#}")))?;
    Ok(Json(results))
}

#[derive(Serialize)]
pub struct GpxImportResp {
    pub points: Vec<LatLng>,
}

// POST /api/gpx/import
pub async fn gpx_import(body: String) -> Result<Json<GpxImportResp>, AppError> {
    let points = parse_gpx_content(&body)
        .map_err(|e| AppError(StatusCode::BAD_REQUEST, format!("GPX parse failed: {e:#}")))?;
    Ok(Json(GpxImportResp { points }))
}

#[derive(Deserialize)]
pub struct GpxExportReq {
    pub points: Vec<LatLng>,
    #[serde(default = "default_gpx_name")]
    pub name: String,
}

fn default_gpx_name() -> String {
    "BloomAnywhere Route".into()
}

// POST /api/gpx/export
pub async fn gpx_export(Json(body): Json<GpxExportReq>) -> Result<Response, AppError> {
    let xml = to_gpx_content(&body.name, &body.points);
    let resp = (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/gpx+xml"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"route.gpx\"",
            ),
        ],
        xml,
    )
        .into_response();
    Ok(resp)
}

// GET /api/favorites
pub async fn list_favorites(State(app_state): State<Arc<AppState>>) -> Json<Vec<Favorite>> {
    Json(app_state.favorites.list())
}

#[derive(Deserialize)]
pub struct AddFavoriteReq {
    pub name: String,
    pub points: Vec<LatLng>,
}

// POST /api/favorites
pub async fn add_favorite(
    State(app_state): State<Arc<AppState>>,
    Json(body): Json<AddFavoriteReq>,
) -> Result<Json<Vec<Favorite>>, AppError> {
    if body.name.trim().is_empty() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Favorite name cannot be empty".into(),
        ));
    }
    let list = app_state.favorites.add(body.name, body.points);
    Ok(Json(list))
}

#[derive(Deserialize)]
pub struct DeleteFavoriteQuery {
    pub id: String,
}

// DELETE /api/favorites
pub async fn delete_favorite(
    State(app_state): State<Arc<AppState>>,
    Query(query): Query<DeleteFavoriteQuery>,
) -> Json<Vec<Favorite>> {
    Json(app_state.favorites.delete(&query.id))
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LocateIpResponse {
    pub lat: f64,
    pub lng: f64,
    pub city: Option<String>,
    pub region: Option<String>,
    pub country: Option<String>,
}

#[derive(Deserialize)]
struct IpWhoIsResp {
    success: Option<bool>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
}

// GET /api/locate/ip
pub async fn locate_ip(
    State(app_state): State<Arc<AppState>>,
) -> Result<Json<LocateIpResponse>, AppError> {
    let url = "https://ipwho.is/";
    if let Ok(res) = app_state
        .http_client
        .get(url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        if res.status().is_success() {
            if let Ok(data) = res.json::<IpWhoIsResp>().await {
                if data.success.unwrap_or(true) {
                    if let (Some(lat), Some(lng)) = (data.latitude, data.longitude) {
                        return Ok(Json(LocateIpResponse {
                            lat,
                            lng,
                            city: data.city,
                            region: data.region,
                            country: data.country,
                        }));
                    }
                }
            }
        }
    }

    // Secondary fallback: ipapi.co
    let url2 = "https://ipapi.co/json/";
    if let Ok(res2) = app_state
        .http_client
        .get(url2)
        .timeout(std::time::Duration::from_secs(4))
        .send()
        .await
    {
        #[derive(Deserialize)]
        struct IpApiResp {
            latitude: Option<f64>,
            longitude: Option<f64>,
            city: Option<String>,
            region: Option<String>,
            country_name: Option<String>,
        }
        if let Ok(data) = res2.json::<IpApiResp>().await {
            if let (Some(lat), Some(lng)) = (data.latitude, data.longitude) {
                return Ok(Json(LocateIpResponse {
                    lat,
                    lng,
                    city: data.city,
                    region: data.region,
                    country: data.country_name,
                }));
            }
        }
    }

    Err(AppError(
        StatusCode::BAD_GATEWAY,
        "Unable to determine location from IP".into(),
    ))
}
