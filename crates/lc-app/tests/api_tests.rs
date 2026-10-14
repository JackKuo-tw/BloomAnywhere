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
