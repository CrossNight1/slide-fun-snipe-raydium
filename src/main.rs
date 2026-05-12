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
mod trades;
mod transaction;
mod types;
mod wallet;
mod web;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{hash::Hash, signer::Signer};
use std::sync::{atomic::AtomicBool, Arc};
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
    let slidefun_wallets = Arc::new(config.enabled_slidefun_keypairs());
    if bundle_wallets.is_empty() {
        log_info!("   [BUNDLE] No enabled sub-wallets — single-wallet mode");
    } else {
        log_info!(
            "   [BUNDLE] {} enabled sub-wallet(s) loaded (Raydium: {} SOL each, SF: {} SOL each)",
            bundle_wallets.len(),
            bundle_wallets.first().map(|w| w.1).unwrap_or(0.0),
            slidefun_wallets.first().map(|w| w.1).unwrap_or(0.0),
        );
        // Initial balance check at startup (removes latency from the snipe path)
        bundle_buy::check_sub_wallet_balances(&config, &bundle_wallets).await;
    }

    let trades = trades::TradesStore::new();
    let bot_active = Arc::new(AtomicBool::new(true)); // Start active by default

    // ── 4. Spawn web dashboard on port 8080 ──────────────────────────────────
    {
        let tx = log_tx.clone();
        let active = bot_active.clone();
        let store = trades.clone();
        tokio::spawn(async move { web::start(tx, active, store).await });
    }

    // ── 5. Build RPC / WebSocket URLs ────────────────────────────────────────
    let rpc_url = config.rpc_url();
    let ws_url = config.ws_url();

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

    // Spawn trade status poller
    {
        let t_poller = trades.clone();
        let r_poller = rpc_client.clone();
        tokio::spawn(async move {
            loop {
                t_poller.poll_pending_trades(&r_poller).await;
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
    }

    // ── 8. Run the dual-listener loop ────────────────────────────────────────
    let state = Arc::new(listener::ListenerState::new(trades));
    listener::run(
        config,
        rpc_client,
        bundle_wallets,
        slidefun_wallets,
        &ws_url,
        state,
        bot_active,
    )
    .await;

    Ok(())
}

fn print_banner(config: &Config) {
    log_info!("╔══════════════════════════════════════════╗");
    log_info!("║   SLIDE-FUN → RAYDIUM SNIPER  v0.2.0    ║");
    log_info!("╚══════════════════════════════════════════╝");
    log_info!("  Wallet      : {}", config.keypair.pubkey());
    log_info!(
        "  Sub-wallets : {}",
        config
            .app
            .bundle_wallets
            .iter()
            .filter(|w| w.enabled)
            .count()
    );
    log_info!("  SOL/main    : {} SOL", config.sol_amount);
    log_info!("  Jito tip    : {} SOL", config.jito_tip);
    log_info!("  Priority fee: {} µ-lam", config.priority_fee);
    let mode = match config.snipe_mode.as_str() {
        "slidefun" => "SLIDEFUN_CREATE",
        "both" => "BOTH (slidefun + raydium)",
        _ => "RAYDIUM_MIGRATE [default]",
    };
    log_info!("  Snipe mode  : {}", mode);
    if config.test_mode {
        log_info!("  ⚡ TEST_MODE  : ON");
    }
    if config.dry_run {
        log_info!("  🧪 DRY RUN   : ON — no real trades");
    } else {
        log_info!("  🚀 LIVE MODE : ON — real trades ENABLED");
    }
    log_info!("  🌐 Dashboard : http://localhost:8080");
}
