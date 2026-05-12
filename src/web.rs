// ==================== WEB DASHBOARD SERVER ====================
//
// Axum HTTP server exposing:
//   GET  /           → serves dashboard HTML
//   GET  /api/config → returns AppConfig (private keys masked)
//   POST /api/config → saves new AppConfig to config.json
//   GET  /api/logs   → Server-Sent Events stream of live log lines

use axum::response::sse::{Event, KeepAlive};
use axum::{
    extract::State,
    http::StatusCode,
    response::{Html, IntoResponse, Sse},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use solana_sdk::signature::SeedDerivable;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
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
        .route("/api/manual-bundle", post(manual_bundle_action))
        .route("/api/explanation", get(get_explanation))
        .route("/api/status", get(get_status).post(set_status))
        .route("/api/check-wallet", post(check_wallet))
        .route("/api/restart", post(restart_bot))
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
async fn set_status(
    State(state): State<AppState>,
    Json(req): Json<StatusReq>,
) -> impl IntoResponse {
    state.bot_active.store(req.active, Ordering::Relaxed);
    let status_str = if req.active { "STARTED" } else { "STOPPED" };
    let _ = state
        .log_tx
        .send(format!("[SYSTEM] Bot engine {}", status_str));
    Json(json!({ "ok": true, "active": req.active }))
}

/// POST /api/restart — exits the process with code 0.
/// Relies on a wrapping bash script (start.sh) to restart it.
async fn restart_bot(State(state): State<AppState>) -> impl IntoResponse {
    let _ = state.log_tx.send("[SYSTEM] 🔄 Restart requested via Dashboard... shutting down now.".to_string());
    
    // Give it a moment to send the log line to the SSE stream
    tokio::spawn(async move {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        std::process::exit(0);
    });

    Json(json!({ "ok": true, "message": "Restarting engine..." }))
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
        match Keypair::try_from(bytes.as_ref()) {
            Ok(k) => k,
            Err(_) => {
                return Json(json!({ "valid": false, "error": "Invalid 64-byte private key" }))
            }
        }
    } else if bytes.len() == 32 {
        let array: [u8; 32] = match bytes.try_into() {
            Ok(a) => a,
            Err(_) => return Json(json!({ "valid": false, "error": "Invalid seed length" })),
        };
        match Keypair::from_seed(&array) {
            Ok(k) => k,
            Err(_) => {
                return Json(json!({ "valid": false, "error": "Failed to derive key from seed" }))
            }
        }
    } else {
        return Json(
            json!({ "valid": false, "error": "Private key must be 64 bytes (secret) or 32 bytes (seed)" }),
        );
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
            Keypair::try_from(bytes.as_ref())
                .ok()
                .map(|k| k.pubkey().to_string())
        } else if bytes.len() == 32 {
            let array: [u8; 32] = bytes.try_into().ok()?;
            Keypair::from_seed(&array)
                .ok()
                .map(|k| k.pubkey().to_string())
        } else {
            None
        }
    };

    // Process main wallet
    if let Some(pk) = val["main_wallet"]["private_key"]
        .as_str()
        .map(|s| s.to_string())
    {
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

    let mut changes = Vec::new();

    if let Some(v) = incoming["helius_api_key"].as_str() {
        if !v.is_empty() && v != existing.helius_api_key {
            existing.helius_api_key = v.to_string();
            changes.push("Helius API Key updated".to_string());
        }
    }
    if let Some(v) = incoming["network"].as_str() {
        if v != existing.network {
            existing.network = v.to_string();
            changes.push(format!("Network -> {}", v));
        }
    }
    if let Some(v) = incoming["snipe_mode"].as_str() {
        if v != existing.snipe_mode {
            existing.snipe_mode = v.to_string();
            changes.push(format!("Mode -> {}", v.to_uppercase()));
        }
    }
    if let Some(v) = incoming["dry_run"].as_bool() {
        if v != existing.dry_run {
            existing.dry_run = v;
            changes.push(format!("Dry Run -> {}", v));
        }
    }
    if let Some(v) = incoming["test_mode"].as_bool() {
        if v != existing.test_mode {
            existing.test_mode = v;
            changes.push(format!("Test Mode -> {}", v));
        }
    }
    if let Some(v) = incoming["jito_tip"].as_f64() {
        if (v - existing.jito_tip).abs() > 0.0000001 {
            existing.jito_tip = v;
            changes.push(format!("Jito Tip -> {} SOL", v));
        }
    }
    if let Some(v) = incoming["cu_limit"].as_u64() {
        if v as u32 != existing.cu_limit {
            existing.cu_limit = v as u32;
            changes.push(format!("CU Limit -> {}", v));
        }
    }
    if let Some(v) = incoming["priority_fee"].as_u64() {
        if v != existing.priority_fee {
            existing.priority_fee = v;
            changes.push(format!("Priority Fee -> {} µ-lam", v));
        }
    }
    if let Some(v) = incoming["auto_snipe_all"].as_bool() {
        if v != existing.auto_snipe_all {
            existing.auto_snipe_all = v;
            changes.push(format!("Auto Snipe All -> {}", v));
        }
    }
    if let Some(v) = incoming["listen_creator"].as_bool() {
        if v != existing.listen_creator {
            existing.listen_creator = v;
            changes.push(format!("Listen Creator -> {}", v));
        }
    }
    if let Some(v) = incoming["slidefun_pump_amount"].as_f64() {
        if (v - existing.slidefun_pump_amount).abs() > 0.0000001 {
            existing.slidefun_pump_amount = v;
            changes.push(format!("Slide.fun Mainnet Buy -> {} SOL", v));
        }
    }
    if let Some(v) = incoming["slidefun_program_mainnet"].as_str() {
        if !v.is_empty() && Some(v.to_string()) != existing.slidefun_program_mainnet {
            existing.slidefun_program_mainnet = Some(v.to_string());
            changes.push(format!("Slide.fun Mainnet Program -> {}", v));
            crate::slidefun_snipe::clear_fee_to_cache();
        }
    }
    if let Some(v) = incoming["slidefun_program_devnet"].as_str() {
        if !v.is_empty() && Some(v.to_string()) != existing.slidefun_program_devnet {
            existing.slidefun_program_devnet = Some(v.to_string());
            changes.push(format!("Slide.fun Devnet Program -> {}", v));
            crate::slidefun_snipe::clear_fee_to_cache();
        }
    }
    if let Some(v) = incoming["raydium_program_mainnet"].as_str() {
        if !v.is_empty() && Some(v.to_string()) != existing.raydium_program_mainnet {
            existing.raydium_program_mainnet = Some(v.to_string());
            changes.push(format!("Raydium Mainnet Program -> {}", v));
        }
    }
    if let Some(v) = incoming["raydium_program_devnet"].as_str() {
        if !v.is_empty() && Some(v.to_string()) != existing.raydium_program_devnet {
            existing.raydium_program_devnet = Some(v.to_string());
            changes.push(format!("Raydium Devnet Program -> {}", v));
        }
    }
    if let Some(v) = incoming["raydium_add_pool_wallet"].as_str() {
        let v_trimmed = v.trim();
        if existing.raydium_add_pool_wallet.as_deref() != Some(v_trimmed) {
            existing.raydium_add_pool_wallet = if v_trimmed.is_empty() {
                None
            } else {
                Some(v_trimmed.to_string())
            };
            changes.push(format!("Raydium Add Pool Wallet -> {}", v_trimmed));
        }
    }
    if let Some(v) = incoming["raydium_add_pool_wallet_devnet"].as_str() {
        let v_trimmed = v.trim();
        if existing.raydium_add_pool_wallet_devnet.as_deref() != Some(v_trimmed) {
            existing.raydium_add_pool_wallet_devnet = if v_trimmed.is_empty() {
                None
            } else {
                Some(v_trimmed.to_string())
            };
            changes.push(format!("Raydium Devnet Add Pool Wallet -> {}", v_trimmed));
        }
    }
    if let Some(v) = incoming["mainnet_rpc"].as_str() {
        if Some(v.to_string()) != existing.mainnet_rpc {
            existing.mainnet_rpc = if v.is_empty() {
                None
            } else {
                Some(v.to_string())
            };
            changes.push(format!("Mainnet RPC -> {}", v));
        }
    }
    if let Some(v) = incoming["mainnet_ws"].as_str() {
        if Some(v.to_string()) != existing.mainnet_ws {
            existing.mainnet_ws = if v.is_empty() {
                None
            } else {
                Some(v.to_string())
            };
            changes.push(format!("Mainnet WS -> {}", v));
        }
    }
    if let Some(v) = incoming["devnet_rpc"].as_str() {
        if Some(v.to_string()) != existing.devnet_rpc {
            existing.devnet_rpc = if v.is_empty() {
                None
            } else {
                Some(v.to_string())
            };
            changes.push(format!("Devnet RPC -> {}", v));
        }
    }
    if let Some(v) = incoming["devnet_ws"].as_str() {
        if Some(v.to_string()) != existing.devnet_ws {
            existing.devnet_ws = if v.is_empty() {
                None
            } else {
                Some(v.to_string())
            };
            changes.push(format!("Devnet WS -> {}", v));
        }
    }
    if let Some(v) = incoming["jito_region"].as_str() {
        if v != existing.jito_region {
            existing.jito_region = v.to_string();
            changes.push(format!("Jito Region -> {}", v));
        }
    }

    // Target Mints (Whitelist)
    if let Some(mints_arr) = incoming["target_mints"].as_array() {
        let new_mints: Vec<String> = mints_arr
            .iter()
            .filter_map(|m| m.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if new_mints != existing.target_mints {
            existing.target_mints = new_mints;
            changes.push(format!(
                "Mainnet Whitelist updated ({} targets)",
                existing.target_mints.len()
            ));
        }
    }
    if let Some(mints_arr) = incoming["target_mints_devnet"].as_array() {
        let new_mints: Vec<String> = mints_arr
            .iter()
            .filter_map(|m| m.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if new_mints != existing.target_mints_devnet {
            existing.target_mints_devnet = new_mints;
            changes.push(format!(
                "Devnet Whitelist updated ({} targets)",
                existing.target_mints_devnet.len()
            ));
        }
    }

    // Target Creators
    if let Some(creators_arr) = incoming["target_creators"].as_array() {
        let new_creators: Vec<String> = creators_arr
            .iter()
            .filter_map(|m| m.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if new_creators != existing.target_creators {
            existing.target_creators = new_creators;
            changes.push(format!(
                "Mainnet Creators updated ({} targets)",
                existing.target_creators.len()
            ));
        }
    }
    if let Some(creators_arr) = incoming["target_creators_devnet"].as_array() {
        let new_creators: Vec<String> = creators_arr
            .iter()
            .filter_map(|m| m.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if new_creators != existing.target_creators_devnet {
            existing.target_creators_devnet = new_creators;
            changes.push(format!(
                "Devnet Creators updated ({} targets)",
                existing.target_creators_devnet.len()
            ));
        }
    }

    // Main wallet
    if let Some(mw) = incoming["main_wallet"].as_object() {
        if let Some(pk) = mw["private_key"].as_str() {
            if !pk.contains("****") && !pk.is_empty() && pk != existing.main_wallet.private_key {
                existing.main_wallet.private_key = pk.to_string();
                changes.push("Main Wallet Key updated".to_string());
            }
        }
        if let Some(amt) = mw["sol_amount"].as_f64() {
            if (amt - existing.main_wallet.sol_amount).abs() > 0.0000001 {
                existing.main_wallet.sol_amount = amt;
                changes.push(format!("Main Wallet Snipe Amount -> {}", amt));
            }
        }
        if let Some(amt) = mw["manual_sol_amount"].as_f64() {
            if (amt - existing.main_wallet.manual_sol_amount).abs() > 0.0000001 {
                existing.main_wallet.manual_sol_amount = amt;
                changes.push(format!("Main Wallet Manual Amount -> {}", amt));
            }
        }
    }

    // Bundle wallets
    if let Some(arr) = incoming["bundle_wallets"].as_array() {
        let mut wallet_changed = false;
        let existing_wallets = existing.bundle_wallets.clone();

        let new_wallets: Vec<_> = arr
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let existing_pk = existing_wallets
                    .get(i)
                    .map(|e| e.private_key.clone())
                    .unwrap_or_default();
                let incoming_pk = w["private_key"].as_str().unwrap_or("");
                let pk = if incoming_pk.contains("****") || incoming_pk.is_empty() {
                    existing_pk
                } else {
                    incoming_pk.to_string()
                };

                crate::config::WalletEntry {
                    label: w["label"]
                        .as_str()
                        .unwrap_or(&format!("Wallet {}", i + 1))
                        .to_string(),
                    private_key: pk,
                    sol_amount: w["sol_amount"].as_f64().unwrap_or(0.05),
                    manual_sol_amount: w["manual_sol_amount"].as_f64().unwrap_or(0.1),
                    enabled: w["enabled"].as_bool().unwrap_or(true),
                }
            })
            .collect();

        if new_wallets.len() != existing.bundle_wallets.len() {
            wallet_changed = true;
        } else {
            for (n, o) in new_wallets.iter().zip(existing.bundle_wallets.iter()) {
                if n.label != o.label
                    || n.private_key != o.private_key
                    || (n.sol_amount - o.sol_amount).abs() > 0.0000001
                    || (n.manual_sol_amount - o.manual_sol_amount).abs() > 0.0000001
                    || n.enabled != o.enabled
                {
                    wallet_changed = true;
                    break;
                }
            }
        }

        if wallet_changed {
            existing.bundle_wallets = new_wallets;
            changes.push(format!(
                "Sub-wallets updated ({} active)",
                existing.bundle_wallets.iter().filter(|w| w.enabled).count()
            ));
        }
    }

    match existing.save() {
        Ok(_) => {
            if changes.is_empty() {
                crate::log_info!("[SYSTEM] Settings saved (no changes).");
            } else {
                for change in changes {
                    crate::log_info!("[SYSTEM] SAVED: {}", change);
                }
            }
            (StatusCode::OK, Json(json!({"ok": true}))).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

/// GET /api/logs — Server-Sent Events stream
async fn stream_logs(
    State(state): State<AppState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = state.log_tx.subscribe();
    let stream = BroadcastStream::new(rx)
        .filter_map(|result| result.ok().map(|line| Ok(Event::default().data(line))));
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn mask_key(key: &str) -> String {
    if key.len() <= 8 || key.starts_with("your_") || key.is_empty() {
        return key.to_string();
    }
    format!("{}****{}", &key[..4], &key[key.len() - 4..])
}

// ─────────────────────────────────────────────
// Manual Action Handlers
// ─────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct WalletAmount {
    label: Option<String>,
    index: Option<usize>,
    amount: f64,
}

#[derive(serde::Deserialize)]
struct ManualBundleReq {
    action: String,
    mint: String,
    percent: Option<f64>,
    amount: Option<f64>,
    wallet_amounts: Option<Vec<WalletAmount>>,
}

async fn manual_bundle_action(Json(req): Json<ManualBundleReq>) -> impl IntoResponse {
    use crate::blockhash::get_blockhash;
    use crate::bundle_buy::{
        raydium_bundle_buy, raydium_bundle_sell, slidefun_bundle_buy, slidefun_bundle_sell,
    };
    use crate::config::Config;
    use crate::pool::find_pool_by_mint;
    use crate::slidefun_snipe::fetch_fee_to;
    use solana_client::nonblocking::rpc_client::RpcClient;
    use solana_sdk::pubkey::Pubkey;
    use solana_sdk::signature::Keypair;
    use std::str::FromStr;
    use std::sync::Arc;

    let config = Config::from_env();
    let mint_pk = match Pubkey::from_str(&req.mint) {
        Ok(pk) => pk,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "Invalid mint address"})),
            )
                .into_response()
        }
    };

    let base_url = if config.network.to_lowercase() == "devnet" {
        "devnet.helius-rpc.com"
    } else {
        "mainnet.helius-rpc.com"
    };
    let rpc_url = format!("https://{}?api-key={}", base_url, config.helius_api_key);
    let rpc = Arc::new(RpcClient::new(rpc_url));

    let mut bundle_wallets: Vec<(Keypair, f64)> = Vec::new();

    if let Some(amounts) = req.wallet_amounts {
        // Use the specific amounts from the Manual tab grid

        for wa in amounts {
            if wa.label == Some("main".to_string()) {
                if let Ok(main_bytes) = bs58::decode(&config.app.main_wallet.private_key).into_vec()
                {
                    if let Ok(kp) = Keypair::try_from(main_bytes.as_ref()) {
                        bundle_wallets.push((kp, wa.amount));
                    }
                }
            } else if let Some(idx) = wa.index {
                if let Some(w) = config.app.bundle_wallets.get(idx) {
                    if let Ok(bytes) = bs58::decode(&w.private_key).into_vec() {
                        if let Ok(kp) = Keypair::try_from(bytes.as_ref()) {
                            bundle_wallets.push((kp, wa.amount));
                        }
                    }
                }
            }
        }
    } else {
        // Fallback to config-defined manual amounts
        bundle_wallets = config.all_manual_keypairs();
    }

    if bundle_wallets.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "No enabled bundle wallets or invalid amounts"})),
        )
            .into_response();
    }

    let action = req.action.clone();
    let percent = req.percent.unwrap_or(100.0);

    tokio::spawn(async move {
        let bh = get_blockhash();
        match action.as_str() {
            "slidefun_buy" => {
                let slidefun_program = config.slidefun_program();
                if let Some(fee_to) = fetch_fee_to(&rpc, &slidefun_program).await {
                    slidefun_bundle_buy(
                        &config,
                        &bundle_wallets,
                        &req.mint,
                        Pubkey::from_str(crate::constants::TOKEN_PROGRAM).unwrap(),
                        &fee_to,
                        bh,
                    )
                    .await;
                }
            }
            "raydium_buy" => {
                if let Some(pool) = find_pool_by_mint(&rpc, &mint_pk, config.raydium_program()).await {
                    raydium_bundle_buy(&config, &bundle_wallets, Arc::new(pool), bh).await;
                }
            }
            "raydium_sell" => {
                if let Some(pool) = find_pool_by_mint(&rpc, &mint_pk, config.raydium_program()).await {
                    raydium_bundle_sell(&config, &bundle_wallets, Arc::new(pool), percent, bh)
                        .await;
                }
            }
            "slidefun_sell" => {
                let slidefun_program = config.slidefun_program();
                if let Some(fee_to) = fetch_fee_to(&rpc, &slidefun_program).await {
                    slidefun_bundle_sell(
                        &config,
                        &bundle_wallets,
                        &req.mint,
                        Pubkey::from_str(crate::constants::TOKEN_PROGRAM).unwrap(),
                        &fee_to,
                        percent,
                        bh,
                    )
                    .await;
                }
            }
            _ => {}
        }
    });

    (
        StatusCode::OK,
        Json(json!({"ok": true, "message": format!("Manual bundle {} started", req.action)})),
    )
        .into_response()
}
