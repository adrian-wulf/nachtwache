mod ai;
mod api;
mod db;
mod ingest;
mod models;
mod web;

use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("setting default subscriber failed");

    let db_path = std::env::var("NACHTWACHE_DB").unwrap_or_else(|_| "nachtwache.db".to_string());
    let port = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(8080);
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

    info!("Initializing Nachtwache SQLite DB at: {}", db_path);
    let db = db::Database::new(&db_path)?;

    // Allow CORS for browser SDKs sending reports
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build router
    let app = Router::new()
        // Sentry SDK Ingest Endpoints
        .route("/api/{project_id}/envelope/", post(ingest::handle_envelope))
        .route("/api/{project_id}/envelope", post(ingest::handle_envelope))
        .route("/api/{project_id}/store/", post(ingest::handle_store))
        .route("/api/{project_id}/store", post(ingest::handle_store))
        // Dashboard Internal API
        .route("/api/issues", get(api::list_issues))
        .route("/api/issues/{id}", get(api::get_issue))
        .route("/api/issues/{id}/status", post(api::update_issue_status))
        .route("/api/issues/{id}/diagnose", post(api::trigger_ai_diagnosis))
        .route("/api/stats", get(api::get_stats))
        .route("/api/settings/ai", get(api::get_ai_settings).post(api::save_ai_settings))
        .route("/api/project", get(api::get_project_info))
        .route("/api/clear", post(api::clear_all_events))
        // State
        .with_state(db)
        .layer(cors)
        // Embedded Dashboard UI
        .fallback(web::static_handler);

    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;

    println!("\n╔══════════════════════════════════════════════════════════════════╗");
    println!("║                   NACHTWACHE  v0.1.0                             ║");
    println!("║     Autonomous AI Error Tracking & Drop-In Sentry Guardian       ║");
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!("► Dashboard:    http://localhost:{}", port);
    println!("► Ingest DSN:   http://public@localhost:{}/1", port);
    println!("► Database:     {}\n", db_path);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
