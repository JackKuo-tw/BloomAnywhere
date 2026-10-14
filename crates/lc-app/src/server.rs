use crate::handlers::*;
use crate::state::AppState;
use crate::ticker::start_ticker;
use crate::ws::ws_handler;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use rust_embed::RustEmbed;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

#[derive(RustEmbed)]
#[folder = "web/"]
pub struct WebAssets;

pub fn create_app(app_state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        // API routes
        .route("/api/devices", get(list_devices))
        .route("/api/connect", post(connect_device))
        .route("/api/disconnect", post(disconnect_device))
        .route("/api/state", get(get_state))
        .route("/api/profiles", get(list_profiles))
        .route("/api/profile", post(set_profile))
        .route("/api/teleport", post(teleport))
        .route("/api/route/plan", post(route_plan))
        .route("/api/route/start", post(route_start))
        .route("/api/route/pause", post(route_pause))
        .route("/api/route/resume", post(route_resume))
        .route("/api/route/stop", post(route_stop))
        .route("/api/speed", post(set_speed))
        .route("/api/joystick", post(set_joystick))
        .route("/api/clear", post(clear_location))
        .route("/api/locate/ip", get(locate_ip))
        .route("/api/search", get(search))
        .route("/api/gpx/import", post(gpx_import))
        .route("/api/gpx/export", post(gpx_export))
        .route(
            "/api/favorites",
            get(list_favorites)
                .post(add_favorite)
                .delete(delete_favorite),
        )
        .route("/api/ws", get(ws_handler))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .fallback(static_file_handler)
        .with_state(app_state)
}

async fn static_file_handler(uri: Uri) -> Response {
    let mut path = uri.path().trim_start_matches('/').to_string();
    if path.is_empty() {
        path = "index.html".to_string();
    }

    match WebAssets::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
        }
        None => {
            if !path.contains('.') {
                if let Some(content) = WebAssets::get("index.html") {
                    return ([(header::CONTENT_TYPE, "text/html")], content.data).into_response();
                }
            }
            (StatusCode::NOT_FOUND, "Not Found").into_response()
        }
    }
}

pub async fn run_server(
    host: &str,
    port: u16,
    open: bool,
    app_state: Arc<AppState>,
) -> anyhow::Result<()> {
    let _ticker_handle = start_ticker(app_state.clone());

    let app = create_app(app_state);

    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let local_addr = listener.local_addr()?;
    let url = format!("http://{}:{}", host, local_addr.port());

    tracing::info!("BloomAnywhere server listening on {url}");
    println!("BloomAnywhere server running at {url}");

    if open {
        let open_url = url.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            if let Err(e) = webbrowser::open(&open_url) {
                tracing::warn!("Failed to open browser: {e:#}");
            }
        });
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    println!("\nShutdown signal received, shutting down gracefully...");
}
