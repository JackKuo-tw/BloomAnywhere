use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use bloomanywhere::server::create_app;
use bloomanywhere::state::{AppState, AppStateView};
use http_body_util::BodyExt;
use lc_core::LatLng;
use lc_device::ConnectOptions;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

fn setup_test_app() -> (axum::Router, Arc<AppState>) {
    let opts = ConnectOptions {
        enable_mock: true,
        ..Default::default()
    };
    let state = AppState::new(opts);
    let app = create_app(state.clone());
    (app, state)
}

#[tokio::test]
async fn test_devices_and_connect_mock() {
    let (app, _state) = setup_test_app();

    // 1. GET /api/devices
    let req = Request::builder()
        .uri("/api/devices")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let devices: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(devices
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["id"] == "mock"));

    // 2. POST /api/connect with "mock"
    let req = Request::builder()
        .method("POST")
        .uri("/api/connect")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "id": "mock" }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();
    assert!(view.connected);
    assert_eq!(view.device.unwrap().id, "mock");

    // 3. GET /api/state
    let req = Request::builder()
        .uri("/api/state")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();
    assert!(view.connected);

    // 4. POST /api/disconnect
    let req = Request::builder()
        .method("POST")
        .uri("/api/disconnect")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();
    assert!(!view.connected);
    assert!(view.device.is_none());
}

#[tokio::test]
async fn test_teleport_and_clear() {
    let (app, _state) = setup_test_app();

    // Connect mock
    let req = Request::builder()
        .method("POST")
        .uri("/api/connect")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "id": "mock" }).to_string()))
        .unwrap();
    app.clone().oneshot(req).await.unwrap();

    // Teleport to Taipei 101
    let target = LatLng::new(25.033964, 121.564468);
    let req = Request::builder()
        .method("POST")
        .uri("/api/teleport")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({ "lat": target.lat, "lng": target.lng }).to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(view.position, Some(target));
    assert_eq!(view.pushes, 1);

    // POST /api/clear
    let req = Request::builder()
        .method("POST")
        .uri("/api/clear")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(view.position, None);
}

#[tokio::test]
async fn test_profiles_and_speed() {
    let (app, _state) = setup_test_app();

    // GET /api/profiles
    let req = Request::builder()
        .uri("/api/profiles")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let profiles: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(profiles
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == "bloom"));

    // POST /api/profile
    let req = Request::builder()
        .method("POST")
        .uri("/api/profile")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "id": "generic" }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(view.profile.id, "generic");

    // POST /api/speed
    let req = Request::builder()
        .method("POST")
        .uri("/api/speed")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(json!({ "speed_kmh": 8.5 }).to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(view.speed_kmh, 8.5);
}

#[tokio::test]
async fn test_route_lifecycle() {
    let (app, _state) = setup_test_app();

    // Route plan
    let p1 = LatLng::new(25.033964, 121.564468);
    let p2 = LatLng::new(25.035000, 121.565000);
    let req = Request::builder()
        .method("POST")
        .uri("/api/route/plan")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "points": [p1, p2],
                "profile": "straight"
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Route start
    let req = Request::builder()
        .method("POST")
        .uri("/api/route/start")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "points": [p1, p2],
                "speed_kmh": 6.0,
                "mode": "loop",
                "snap": "straight"
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();
    assert!(view.route.is_some());
    assert!(!view.route.unwrap().paused);

    // Route pause
    let req = Request::builder()
        .method("POST")
        .uri("/api/route/pause")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();
    assert!(view.route.unwrap().paused);

    // Route resume
    let req = Request::builder()
        .method("POST")
        .uri("/api/route/resume")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();
    assert!(!view.route.unwrap().paused);

    // Route stop
    let req = Request::builder()
        .method("POST")
        .uri("/api/route/stop")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();
    assert!(view.route.is_none());
}

#[tokio::test]
async fn test_route_with_lead_in() {
    let (app, _state) = setup_test_app();

    let center = LatLng::new(25.033000, 121.565000);
    let p1 = LatLng::new(25.034000, 121.565000);
    let p2 = LatLng::new(25.034000, 121.566000);

    // Route start with lead_in from center to p1, then loop between p1 and p2
    let req = Request::builder()
        .method("POST")
        .uri("/api/route/start")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "points": [p1, p2],
                "lead_in": [center, p1],
                "speed_kmh": 5.5,
                "mode": "loop",
                "snap": "straight"
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();

    // Verify ZERO teleportation: initial position must strictly be center!
    assert_eq!(view.position, Some(center));
    let route = view.route.expect("route exists");
    assert_eq!(route.lead_in.len(), 2);
    assert_eq!(route.lead_in[0], center);
    assert_eq!(route.lead_in[1], p1);
}

#[tokio::test]
async fn test_gpx_import_and_export() {
    let (app, _state) = setup_test_app();

    let gpx_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="Test">
  <trk>
    <trkseg>
      <trkpt lat="25.033964" lon="121.564468"></trkpt>
      <trkpt lat="25.047675" lon="121.517055"></trkpt>
    </trkseg>
  </trk>
</gpx>"#;

    // Import GPX
    let req = Request::builder()
        .method("POST")
        .uri("/api/gpx/import")
        .header(header::CONTENT_TYPE, "application/xml")
        .body(Body::from(gpx_xml))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let parsed: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(parsed["points"].as_array().unwrap().len(), 2);

    // Export GPX
    let p1 = LatLng::new(25.033964, 121.564468);
    let req = Request::builder()
        .method("POST")
        .uri("/api/gpx/export")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "name": "Exported Route",
                "points": [p1]
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/gpx+xml"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let xml_str = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(xml_str.contains("<trkpt"));
    let parsed_back = bloomanywhere::gpx_util::parse_gpx_content(&xml_str).unwrap();
    assert_eq!(parsed_back.len(), 1);
    assert!((parsed_back[0].lat - p1.lat).abs() < 1e-5);
    assert!((parsed_back[0].lng - p1.lng).abs() < 1e-5);
}

#[tokio::test]
async fn test_favorites_crud() {
    let (app, _state) = setup_test_app();

    // 1. POST /api/favorites
    let p1 = LatLng::new(25.033964, 121.564468);
    let req = Request::builder()
        .method("POST")
        .uri("/api/favorites")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "name": "Taipei 101 Test",
                "points": [p1]
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let favs: Value = serde_json::from_slice(&bytes).unwrap();
    let fav = favs
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "Taipei 101 Test")
        .unwrap();
    let fav_id = fav["id"].as_str().unwrap().to_string();

    // 2. GET /api/favorites
    let req = Request::builder()
        .uri("/api/favorites")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 3. DELETE /api/favorites?id=...
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/api/favorites?id={fav_id}"))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let favs: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(!favs.as_array().unwrap().iter().any(|f| f["id"] == fav_id));
}

#[tokio::test]
async fn test_joystick_control() {
    let (app, _state) = setup_test_app();

    let req = Request::builder()
        .method("POST")
        .uri("/api/joystick")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "active": true,
                "heading_deg": 90.0
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(view.mode, bloomanywhere::state::AppMode::Joystick);
    assert_eq!(view.heading_deg, 90.0);

    // Deactivate joystick
    let req = Request::builder()
        .method("POST")
        .uri("/api/joystick")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            json!({
                "active": false,
                "heading_deg": 90.0
            })
            .to_string(),
        ))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let view: AppStateView = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(view.mode, bloomanywhere::state::AppMode::Idle);
}

#[tokio::test]
async fn test_static_files_serving() {
    let (app, _state) = setup_test_app();

    // GET / serves index.html
    let req = Request::builder().uri("/").body(Body::empty()).unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get(header::CONTENT_TYPE).unwrap(),
        "text/html"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let html = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(html.contains("BloomAnywhere"));

    // GET /vendor/leaflet/leaflet.js serves leaflet JS
    let req = Request::builder()
        .uri("/vendor/leaflet/leaflet.js")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_locate_ip_endpoint_routed() {
    let (app, _state) = setup_test_app();
    let req = Request::builder()
        .uri("/api/locate/ip")
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert!(
        resp.status() == StatusCode::OK || resp.status() == StatusCode::BAD_GATEWAY,
        "unexpected status: {}",
        resp.status()
    );
}

async fn post_json(app: &axum::Router, uri: &str, body: Value) -> AppStateView {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn post_empty(app: &axum::Router, uri: &str) -> AppStateView {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn test_route_pause_manual_move_and_resume_at_pause_point() {
    let (app, state) = setup_test_app();
    let p1 = LatLng::new(25.033964, 121.564468);
    let p2 = LatLng::new(25.035000, 121.564468);

    // 1. Start route
    post_json(
        &app,
        "/api/route/start",
        json!({
            "points": [p1, p2],
            "speed_kmh": 36.0,
            "mode": "once",
            "snap": "straight"
        }),
    )
    .await;

    // Advance 2s (20m along route)
    {
        let mut inner = state.inner.write().await;
        let tick = inner
            .active_route
            .as_mut()
            .unwrap()
            .step(std::time::Duration::from_secs(2));
        inner.position = Some(tick.position);
    }

    // 2. Pause -> sets temporary pause_point
    let view = post_empty(&app, "/api/route/pause").await;
    let route = view.route.expect("route exists");
    assert!(route.paused);
    let pause_pt = route.pause_point.expect("pause_point should be marked");

    // 3. User manually moves with joystick then releases (mode becomes Idle)
    let view = post_json(
        &app,
        "/api/joystick",
        json!({ "active": true, "heading_deg": 90.0 }),
    )
    .await;
    assert_eq!(view.mode, bloomanywhere::state::AppMode::Joystick);
    let view = post_json(
        &app,
        "/api/joystick",
        json!({ "active": false, "heading_deg": 90.0 }),
    )
    .await;
    assert_eq!(view.mode, bloomanywhere::state::AppMode::Idle);

    // 4. User manually returns right back to pause_pt (within 4m) and resumes
    post_json(
        &app,
        "/api/teleport",
        json!({ "lat": pause_pt.lat, "lng": pause_pt.lng }),
    )
    .await;

    let view = post_empty(&app, "/api/route/resume").await;
    // Must switch back to AppMode::Route instead of freezing in Teleport/Idle!
    assert_eq!(view.mode, bloomanywhere::state::AppMode::Route);
    let route = view.route.expect("route exists");
    assert!(!route.paused);
    assert!(!route.returning_to_pause);
    assert!(route.pause_point.is_none());
}

#[tokio::test]
async fn test_route_pause_walk_away_then_resume_walks_back_to_pause_point() {
    let (app, state) = setup_test_app();
    let p1 = LatLng::new(25.033964, 121.564468);
    let p2 = LatLng::new(25.036000, 121.564468);

    // Start route at 36 km/h (10 m/s)
    post_json(
        &app,
        "/api/route/start",
        json!({
            "points": [p1, p2],
            "speed_kmh": 36.0,
            "mode": "once",
            "snap": "straight"
        }),
    )
    .await;

    // Advance 2s (20m north)
    {
        let mut inner = state.inner.write().await;
        let tick = inner
            .active_route
            .as_mut()
            .unwrap()
            .step(std::time::Duration::from_secs(2));
        inner.position = Some(tick.position);
    }

    // Pause and verify pause_point
    let view = post_empty(&app, "/api/route/pause").await;
    let route = view.route.unwrap();
    let pause_pt = route.pause_point.unwrap();
    let paused_progress = route.lap_progress_m;
    assert!((paused_progress - 20.0).abs() < 0.1);

    // Move ~50m east away from pause_pt
    let off_route = lc_core::geo::destination(pause_pt, 90.0, 50.0);
    post_json(
        &app,
        "/api/teleport",
        json!({ "lat": off_route.lat, "lng": off_route.lng }),
    )
    .await;

    // Resume: should plan return path from off_route back to pause_pt
    let view = post_empty(&app, "/api/route/resume").await;
    assert_eq!(view.mode, bloomanywhere::state::AppMode::Route);
    let route = view.route.unwrap();
    assert!(!route.paused);
    assert!(route.returning_to_pause);
    assert_eq!(route.pause_point, Some(pause_pt));
    assert!(route.return_path.len() >= 2);
    assert!(lc_core::geo::haversine_m(route.return_path[0], off_route) < 0.5);
    assert!(lc_core::geo::haversine_m(*route.return_path.last().unwrap(), pause_pt) < 0.5);

    // Step 2s (20m of 50m): still returning, main route progress frozen
    {
        let mut inner = state.inner.write().await;
        let tick = inner
            .active_route
            .as_mut()
            .unwrap()
            .step(std::time::Duration::from_secs(2));
        inner.position = Some(tick.position);
        let view = inner.to_view();
        let r = view.route.unwrap();
        assert!(r.returning_to_pause);
        assert!((r.lap_progress_m - paused_progress).abs() < 0.01);
        assert!(lc_core::geo::haversine_m(tick.position, pause_pt) < 31.0);
    }

    // Step 3.5s (35m): arrives at pause_pt, clears return state, resumes main route
    {
        let mut inner = state.inner.write().await;
        let tick = inner
            .active_route
            .as_mut()
            .unwrap()
            .step(std::time::Duration::from_millis(3500));
        inner.position = Some(tick.position);
        let view = inner.to_view();
        let r = view.route.unwrap();
        assert!(!r.returning_to_pause);
        assert!(r.pause_point.is_none());
        assert!(r.return_path.is_empty());
        assert!(!r.paused);
    }

    // Step 1s more: main route now advances past paused_progress!
    {
        let mut inner = state.inner.write().await;
        let tick = inner
            .active_route
            .as_mut()
            .unwrap()
            .step(std::time::Duration::from_secs(1));
        inner.position = Some(tick.position);
        let view = inner.to_view();
        let r = view.route.unwrap();
        assert!(r.lap_progress_m > paused_progress + 9.0);
    }
}

#[tokio::test]
async fn test_joystick_autopauses_route_and_preserves_pause_point_mid_return() {
    let (app, state) = setup_test_app();
    let p1 = LatLng::new(25.033964, 121.564468);
    let p2 = LatLng::new(25.036000, 121.564468);

    post_json(
        &app,
        "/api/route/start",
        json!({
            "points": [p1, p2],
            "speed_kmh": 36.0,
            "mode": "once",
            "snap": "straight"
        }),
    )
    .await;

    // Advance 1s along route
    {
        let mut inner = state.inner.write().await;
        let tick = inner
            .active_route
            .as_mut()
            .unwrap()
            .step(std::time::Duration::from_secs(1));
        inner.position = Some(tick.position);
    }

    // Activate joystick directly without clicking Pause first
    let view = post_json(
        &app,
        "/api/joystick",
        json!({ "active": true, "heading_deg": 90.0 }),
    )
    .await;
    assert_eq!(view.mode, bloomanywhere::state::AppMode::Joystick);
    let route = view.route.unwrap();
    assert!(route.paused);
    let original_pause_pt = route.pause_point.expect("auto-marked pause point");

    // Release joystick and move 40m away
    post_json(
        &app,
        "/api/joystick",
        json!({ "active": false, "heading_deg": 90.0 }),
    )
    .await;
    let away1 = lc_core::geo::destination(original_pause_pt, 90.0, 40.0);
    post_json(
        &app,
        "/api/teleport",
        json!({ "lat": away1.lat, "lng": away1.lng }),
    )
    .await;

    // Resume -> returning_to_pause
    let view = post_empty(&app, "/api/route/resume").await;
    assert!(view.route.unwrap().returning_to_pause);

    // Pause mid-return and verify original pause point is preserved
    let view = post_empty(&app, "/api/route/pause").await;
    let route = view.route.unwrap();
    assert!(route.paused);
    assert_eq!(route.pause_point, Some(original_pause_pt));

    // Move further away while paused mid-return, then resume -> replans to original_pause_pt
    let away2 = lc_core::geo::destination(original_pause_pt, 180.0, 60.0);
    post_json(
        &app,
        "/api/teleport",
        json!({ "lat": away2.lat, "lng": away2.lng }),
    )
    .await;
    let view = post_empty(&app, "/api/route/resume").await;
    assert_eq!(view.mode, bloomanywhere::state::AppMode::Route);
    let route = view.route.unwrap();
    assert!(route.returning_to_pause);
    assert_eq!(route.pause_point, Some(original_pause_pt));
    assert!(lc_core::geo::haversine_m(route.return_path[0], away2) < 0.5);
    assert!(lc_core::geo::haversine_m(*route.return_path.last().unwrap(), original_pause_pt) < 0.5);
}
