mod blockhash;
mod bundle_buy;
mod config;
mod constants;
mod graduation;
mod handler;
mod listener;
mod logger;
mod pool;
mod slidefun_snipe;
mod transaction;
mod types;
mod wallet;
mod web;

use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{hash::Hash, signer::Signer};
use tokio::time::{sleep, Duration};

use blockhash::{blockhash_updater, get_blockhash};
use config::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── 1. Init logger + broadcast channel (for SSE log streaming) ───────────
    let log_tx = logger::init_logger(512);

    // ── 2. Load unified config.json (or .env fallback) ───────────────────────
    let config = Arc::new(Config::from_env());
    print_banner(&config);

    // ── 3. Load enabled bundle wallets ───────────────────────────────────────
    let bundle_wallets = Arc::new(config.enabled_bundle_keypairs());
    if bundle_wallets.is_empty() {
        log_info!("   [BUNDLE] No enabled sub-wallets — single-wallet mode");
    } else {
        log_info!("   [BUNDLE] {} enabled sub-wallet(s) loaded", bundle_wallets.len());
    }

    let bot_active = Arc::new(AtomicBool::new(false)); // Start paused

    // ── 4. Spawn web dashboard on port 8080 ──────────────────────────────────
    {
        let tx = log_tx.clone();
        let active = bot_active.clone();
        tokio::spawn(async move { web::start(tx, active).await });
    }

    // ── 5. Build RPC / WebSocket URLs ────────────────────────────────────────
    let rpc_url = format!(
        "https://mainnet.helius-rpc.com/?api-key={}",
        config.helius_api_key
    );
    let ws_url = format!(
        "wss://mainnet.helius-rpc.com/?api-key={}",
        config.helius_api_key
    );

    // ── 6. Start background blockhash updater ────────────────────────────────
    {
        let url = rpc_url.clone();
        tokio::spawn(async move { blockhash_updater(url).await });
    }
    while get_blockhash() == Hash::default() {
        sleep(Duration::from_millis(10)).await;
    }
    log_info!("[OK] Blockhash cache ready");

    // ── 7. Pre-fund WSOL account (live mode only) ────────────────────────────
    let rpc_client = Arc::new(RpcClient::new(rpc_url));
    wallet::prefund_wsol(&config, &rpc_client).await;

    // ── 8. Run the dual-listener loop ────────────────────────────────────────
    let state = listener::ListenerState::new();
    listener::run(config, rpc_client, bundle_wallets, &ws_url, &state, bot_active).await;

    Ok(())
}

fn print_banner(config: &Config) {
    log_info!("╔══════════════════════════════════════════╗");
    log_info!("║   SLIDE-FUN → RAYDIUM SNIPER  v0.2.0    ║");
    log_info!("╚══════════════════════════════════════════╝");
    log_info!("  Wallet      : {}", config.keypair.pubkey());
    log_info!("  Sub-wallets : {}", config.app.bundle_wallets.iter().filter(|w| w.enabled).count());
    log_info!("  SOL/main    : {} SOL", config.sol_amount);
    log_info!("  Jito tip    : {} SOL", config.jito_tip);
    log_info!("  Priority fee: {} µ-lam", config.priority_fee);
    let mode = match config.snipe_mode.as_str() {
        "slidefun" => "SLIDEFUN_CREATE",
        "both"     => "BOTH (slidefun + raydium)",
        _          => "RAYDIUM_MIGRATE [default]",
    };
    log_info!("  Snipe mode  : {}", mode);
    if config.test_mode { log_info!("  ⚡ TEST_MODE  : ON"); }
    if config.dry_run   { log_info!("  🧪 DRY RUN   : ON — no real trades"); }
    else                { log_info!("  🚀 LIVE MODE : ON — real trades ENABLED"); }
    log_info!("  🌐 Dashboard : http://localhost:8080");
}
