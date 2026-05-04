// ==================== WEB DASHBOARD SERVER ====================
//
// Axum HTTP server exposing:
//   GET  /           → serves dashboard HTML
//   GET  /api/config → returns AppConfig (private keys masked)
//   POST /api/config → saves new AppConfig to config.json
//   GET  /api/logs   → Server-Sent Events stream of live log lines

use std::sync::Arc;
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Sse},
    routing::{get, post},
    Json, Router,
};
use axum::response::sse::{Event, KeepAlive};
use serde_json::{json, Value};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt as _;

use crate::config::AppConfig;

pub type LogTx = broadcast::Sender<String>;

#[derive(Clone)]
pub struct AppState {
    pub log_tx: LogTx,
}

/// Start the dashboard on port 8080. Call from main with tokio::spawn.
pub async fn start(log_tx: LogTx) {
    let state = AppState { log_tx };
    let app = Router::new()
        .route("/", get(serve_dashboard))
        .route("/api/config", get(get_config))
        .route("/api/config", post(save_config))
        .route("/api/logs", get(stream_logs))
        .with_state(state);

    let addr = "0.0.0.0:8080";
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("[WEB] Dashboard running at http://localhost:8080");
    axum::serve(listener, app).await.unwrap();
}

/// GET / — inline HTML dashboard
async fn serve_dashboard() -> impl IntoResponse {
    Html(include_str!("../dashboard/index.html"))
}

/// GET /api/config — return config with private keys masked
async fn get_config() -> impl IntoResponse {
    let cfg = AppConfig::load();
    let mut val = serde_json::to_value(&cfg).unwrap();

    // Mask private keys for safety
    if let Some(pk) = val["main_wallet"]["private_key"].as_str() {
        val["main_wallet"]["private_key"] = json!(mask_key(pk));
    }
    if let Some(arr) = val["bundle_wallets"].as_array_mut() {
        for w in arr.iter_mut() {
            if let Some(pk) = w["private_key"].as_str() {
                w["private_key"] = json!(mask_key(pk));
            }
        }
    }
    Json(val)
}

/// POST /api/config — save new config (only updates non-masked keys)
async fn save_config(Json(incoming): Json<Value>) -> impl IntoResponse {
    // Load existing config to preserve masked private keys
    let mut existing = AppConfig::load();

    macro_rules! patch_str { ($field:expr, $key:expr) => {
        if let Some(v) = $key.as_str() { if !v.is_empty() { $field = v.to_string(); } }
    };}
    macro_rules! patch_f64 { ($field:expr, $key:expr) => {
        if let Some(v) = $key.as_f64() { $field = v; }
    };}
    macro_rules! patch_u64 { ($field:expr, $key:expr) => {
        if let Some(v) = $key.as_u64() { $field = v; }
    };}
    macro_rules! patch_u32 { ($field:expr, $key:expr) => {
        if let Some(v) = $key.as_u64() { $field = v as u32; }
    };}
    macro_rules! patch_bool { ($field:expr, $key:expr) => {
        if let Some(v) = $key.as_bool() { $field = v; }
    };}

    patch_str!(existing.helius_api_key, incoming["helius_api_key"]);
    patch_str!(existing.snipe_mode, incoming["snipe_mode"]);
    patch_bool!(existing.dry_run, incoming["dry_run"]);
    patch_bool!(existing.test_mode, incoming["test_mode"]);
    patch_f64!(existing.jito_tip, incoming["jito_tip"]);
    patch_u32!(existing.cu_limit, incoming["cu_limit"]);
    patch_u64!(existing.priority_fee, incoming["priority_fee"]);
    patch_f64!(existing.slidefun_pump_amount, incoming["slidefun_pump_amount"]);

    // Target Mints (Whitelist)
    if let Some(mints) = incoming["target_mints"].as_array() {
        existing.target_mints = mints.iter()
            .filter_map(|m| m.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }

    // Main wallet
    if let Some(mw) = incoming["main_wallet"].as_object() {
        if let Some(label) = mw["label"].as_str() { existing.main_wallet.label = label.to_string(); }
        if let Some(pk) = mw["private_key"].as_str() {
            if !pk.contains("****") && !pk.is_empty() {
                existing.main_wallet.private_key = pk.to_string();
            }
        }
        if let Some(sol) = mw["sol_amount"].as_f64() { existing.main_wallet.sol_amount = sol; }
        if let Some(en) = mw["enabled"].as_bool() { existing.main_wallet.enabled = en; }
    }

    // Bundle wallets — replace entire list
    if let Some(arr) = incoming["bundle_wallets"].as_array() {
        let existing_wallets = existing.bundle_wallets.clone();
        existing.bundle_wallets = arr.iter().enumerate().map(|(i, w)| {
            let existing_pk = existing_wallets.get(i)
                .map(|e| e.private_key.clone())
                .unwrap_or_default();
            let incoming_pk = w["private_key"].as_str().unwrap_or("");
            let pk = if incoming_pk.contains("****") || incoming_pk.is_empty() {
                existing_pk
            } else {
                incoming_pk.to_string()
            };
            crate::config::WalletEntry {
                label: w["label"].as_str().unwrap_or(&format!("Wallet {}", i+1)).to_string(),
                private_key: pk,
                sol_amount: w["sol_amount"].as_f64().unwrap_or(0.05),
                enabled: w["enabled"].as_bool().unwrap_or(true),
            }
        }).collect();
    }

    match existing.save() {
        Ok(_) => (StatusCode::OK, Json(json!({"ok": true}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

/// GET /api/logs — Server-Sent Events stream
async fn stream_logs(State(state): State<AppState>) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.log_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| {
        result.ok().map(|line| Ok(Event::default().data(line)))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 || key.starts_with("your_") || key.is_empty() {
        return key.to_string();
    }
    format!("{}****{}", &key[..4], &key[key.len()-4..])
}
