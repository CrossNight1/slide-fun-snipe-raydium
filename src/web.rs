// ==================== WEB DASHBOARD SERVER ====================
//
// Axum HTTP server exposing:
//   GET  /           → serves dashboard HTML
//   GET  /api/config → returns AppConfig (private keys masked)
//   POST /api/config → saves new AppConfig to config.json
//   GET  /api/logs   → Server-Sent Events stream of live log lines

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use solana_sdk::signature::SeedDerivable;
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
    pub bot_active: Arc<AtomicBool>,
}

/// Start the dashboard on port 8080. Call from main with tokio::spawn.
pub async fn start(log_tx: LogTx, bot_active: Arc<AtomicBool>) {
    let state = AppState { log_tx, bot_active };
    let app = Router::new()
        .route("/", get(serve_dashboard))
        .route("/api/config", get(get_config))
        .route("/api/config", post(save_config))
        .route("/api/logs", get(stream_logs))
        .route("/api/manual-buy", post(manual_bundle_buy))
        .route("/api/manual-sell", post(manual_bundle_sell))
        .route("/api/explanation", get(get_explanation))
        .route("/api/status", get(get_status).post(set_status))
        .route("/api/check-wallet", post(check_wallet))
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

/// GET /api/status — returns {"active": true/false}
async fn get_status(State(state): State<AppState>) -> impl IntoResponse {
    let active = state.bot_active.load(Ordering::Relaxed);
    Json(json!({ "active": active }))
}

/// POST /api/status — set bot active state
#[derive(serde::Deserialize)]
struct StatusReq {
    active: bool,
}
async fn set_status(State(state): State<AppState>, Json(req): Json<StatusReq>) -> impl IntoResponse {
    state.bot_active.store(req.active, Ordering::Relaxed);
    let status_str = if req.active { "STARTED" } else { "STOPPED" };
    let _ = state.log_tx.send(format!("[SYSTEM] Bot engine {}", status_str));
    Json(json!({ "ok": true, "active": req.active }))
}

/// POST /api/check-wallet — validate a private key and return pubkey
#[derive(serde::Deserialize)]
struct CheckWalletReq {
    private_key: String,
}
async fn check_wallet(Json(req): Json<CheckWalletReq>) -> impl IntoResponse {
    use solana_sdk::signature::Keypair;
    use solana_sdk::signer::Signer;
    
    let bytes = match bs58::decode(&req.private_key).into_vec() {
        Ok(b) => b,
        Err(_) => return Json(json!({ "valid": false, "error": "Invalid Base58 format" })),
    };

    let keypair = if bytes.len() == 64 {
        match Keypair::from_bytes(&bytes) {
            Ok(k) => k,
            Err(_) => return Json(json!({ "valid": false, "error": "Invalid 64-byte private key" })),
        }
    } else if bytes.len() == 32 {
        let array: [u8; 32] = match bytes.try_into() {
            Ok(a) => a,
            Err(_) => return Json(json!({ "valid": false, "error": "Invalid seed length" })),
        };
        match Keypair::from_seed(&array) {
            Ok(k) => k,
            Err(_) => return Json(json!({ "valid": false, "error": "Failed to derive key from seed" })),
        }
    } else {
        return Json(json!({ "valid": false, "error": "Private key must be 64 bytes (secret) or 32 bytes (seed)" }));
    };

    Json(json!({
        "valid": true,
        "pubkey": keypair.pubkey().to_string()
    }))
}

/// GET /api/explanation — return content of explanation.md
async fn get_explanation() -> impl IntoResponse {
    match std::fs::read_to_string("explanation.md") {
        Ok(content) => content,
        Err(_) => "# Explanation file not found.\nPlease ensure `explanation.md` exists in the root directory.".to_string(),
    }
}

/// GET /api/config — return config with private keys masked and pubkeys attached
async fn get_config() -> impl IntoResponse {
    use solana_sdk::signature::Keypair;
    use solana_sdk::signer::Signer;

    let cfg = AppConfig::load();
    let mut val = serde_json::to_value(&cfg).unwrap();

    // Helper to derive pubkey
    let derive_pubkey = |pk_str: &str| -> Option<String> {
        let bytes = bs58::decode(pk_str).into_vec().ok()?;
        if bytes.len() == 64 {
            Keypair::from_bytes(&bytes).ok().map(|k| k.pubkey().to_string())
        } else if bytes.len() == 32 {
            let array: [u8; 32] = bytes.try_into().ok()?;
            Keypair::from_seed(&array).ok().map(|k| k.pubkey().to_string())
        } else {
            None
        }
    };

    // Process main wallet
    if let Some(pk) = val["main_wallet"]["private_key"].as_str().map(|s| s.to_string()) {
        if let Some(pubkey) = derive_pubkey(&pk) {
            val["main_wallet"]["pubkey"] = json!(pubkey);
        }
        val["main_wallet"]["private_key"] = json!(mask_key(&pk));
    }

    // Process sub-wallets
    if let Some(arr) = val["bundle_wallets"].as_array_mut() {
        for w in arr.iter_mut() {
            if let Some(pk) = w["private_key"].as_str().map(|s| s.to_string()) {
                if let Some(pubkey) = derive_pubkey(&pk) {
                    w["pubkey"] = json!(pubkey);
                }
                w["private_key"] = json!(mask_key(&pk));
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
    patch_str!(existing.network, incoming["network"]);
    patch_str!(existing.snipe_mode, incoming["snipe_mode"]);
    patch_bool!(existing.dry_run, incoming["dry_run"]);
    patch_bool!(existing.test_mode, incoming["test_mode"]);
    patch_f64!(existing.jito_tip, incoming["jito_tip"]);
    patch_u32!(existing.cu_limit, incoming["cu_limit"]);
    patch_u64!(existing.priority_fee, incoming["priority_fee"]);
    patch_bool!(existing.auto_snipe_all, incoming["auto_snipe_all"]);
    patch_f64!(existing.slidefun_pump_amount, incoming["slidefun_pump_amount"]);

    // Target Mints (Whitelist)
    if let Some(mints) = incoming["target_mints"].as_array() {
        existing.target_mints = mints.iter()
            .filter_map(|m| m.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
    }

    // Target Creators
    if let Some(creators) = incoming["target_creators"].as_array() {
        existing.target_creators = creators.iter()
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

// ─────────────────────────────────────────────
// Manual Action Handlers
// ─────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct ManualBuyReq {
    mint: String,
    sol_per_wallet: f64,
}

#[derive(serde::Deserialize)]
struct ManualSellReq {
    mint: String,
    percent: f64,
}

async fn manual_bundle_buy(Json(req): Json<ManualBuyReq>) -> impl IntoResponse {
    use crate::config::Config;
    use crate::pool::find_pool_by_mint;
    use crate::bundle_buy::raydium_bundle_buy;
    use crate::blockhash::get_blockhash;
    use solana_sdk::pubkey::Pubkey;
    use solana_client::nonblocking::rpc_client::RpcClient;
    use std::str::FromStr;

    let config = Config::from_env();
    let mint = match Pubkey::from_str(&req.mint) {
        Ok(pk) => pk,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid mint address"}))).into_response(),
    };

    let base_url = if config.network.to_lowercase() == "devnet" {
        "devnet.helius-rpc.com"
    } else {
        "mainnet.helius-rpc.com"
    };
    let rpc_url = format!("https://{}?api-key={}", base_url, config.helius_api_key);
    let rpc = RpcClient::new(rpc_url);

    let bundle_wallets = config.enabled_bundle_keypairs();
    if bundle_wallets.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "No enabled bundle wallets"}))).into_response();
    }
    let override_wallets: Vec<_> = bundle_wallets.into_iter().map(|(k, _)| (k, req.sol_per_wallet)).collect();

    // Run in background to avoid blocking web server
    tokio::spawn(async move {
        if let Some(pool) = find_pool_by_mint(&rpc, &mint).await {
            let bh = get_blockhash();
            raydium_bundle_buy(&config, &override_wallets, Arc::new(pool), bh).await;
        }
    });

    (StatusCode::OK, Json(json!({"ok": true, "message": "Manual bundle buy started"}))).into_response()
}

async fn manual_bundle_sell(Json(req): Json<ManualSellReq>) -> impl IntoResponse {
    use crate::config::Config;
    use crate::pool::find_pool_by_mint;
    use crate::bundle_buy::raydium_bundle_sell;
    use crate::blockhash::get_blockhash;
    use solana_sdk::pubkey::Pubkey;
    use solana_client::nonblocking::rpc_client::RpcClient;
    use std::str::FromStr;

    let config = Config::from_env();
    let mint = match Pubkey::from_str(&req.mint) {
        Ok(pk) => pk,
        Err(_) => return (StatusCode::BAD_REQUEST, Json(json!({"error": "Invalid mint address"}))).into_response(),
    };

    let base_url = if config.network.to_lowercase() == "devnet" {
        "devnet.helius-rpc.com"
    } else {
        "mainnet.helius-rpc.com"
    };
    let rpc_url = format!("https://{}?api-key={}", base_url, config.helius_api_key);
    let rpc = RpcClient::new(rpc_url);

    let bundle_wallets = config.enabled_bundle_keypairs();
    if bundle_wallets.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": "No enabled bundle wallets"}))).into_response();
    }

    tokio::spawn(async move {
        if let Some(pool) = find_pool_by_mint(&rpc, &mint).await {
            let bh = get_blockhash();
            raydium_bundle_sell(&config, &bundle_wallets, Arc::new(pool), req.percent, bh).await;
        }
    });

    (StatusCode::OK, Json(json!({"ok": true, "message": "Manual bundle sell started"}))).into_response()
}
