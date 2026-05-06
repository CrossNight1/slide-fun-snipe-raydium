// ==================== DUAL-LISTENER ====================
//
// Handles the two concurrent WebSocket subscriptions:
//
//   Listener A (Slide.fun program)
//     - Detects `CreateBondingCurve` → immediate bonding-curve snipe (slidefun mode)
//     - Detects `Migrate`            → adds token to graduation watch-list + pre-creates ATA
//
//   Listener B (Raydium AMM V4)
//     - Detects `initialize2`        → if token is in watch-list → fire swap (raydium mode)
//
// The listener loop reconnects automatically on WebSocket disconnection.

use std::{collections::HashSet, str::FromStr, sync::{Arc, atomic::{AtomicBool, Ordering}}};

use futures_util::StreamExt;
use solana_client::{
    nonblocking::{pubsub_client::PubsubClient, rpc_client::RpcClient},
    rpc_config::{RpcTransactionLogsConfig, RpcTransactionLogsFilter},
};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey, signer::keypair::Keypair};
use tokio::{
    sync::Mutex,
    time::{sleep, Duration},
};

use crate::{
    blockhash::get_blockhash,
    bundle_buy,
    config::Config,
    constants,
    graduation,
    handler::handle_buy,
    log_info,
    pool::get_pool_info,
    slidefun_snipe,
};

/// Shared state passed into the listener loop.
pub struct ListenerState {
    /// Tokens detected as graduating from Slide.fun (migrate step seen)
    pub graduating_tokens: Arc<Mutex<HashSet<String>>>,
    /// Tokens whose ATA has already been pre-created during the migrate step
    pub ata_pre_created_tokens: Arc<Mutex<HashSet<String>>>,
    /// Deduplication: tokens already sniped (avoids double-buying the same token)
    pub sniped_tokens: Arc<Mutex<HashSet<String>>>,
    /// Tokens tracked from specific creators (listen_creator mode)
    pub creator_tracked_tokens: Arc<Mutex<HashSet<String>>>,
}

impl ListenerState {
    pub fn new() -> Self {
        Self {
            graduating_tokens: Arc::new(Mutex::new(HashSet::new())),
            ata_pre_created_tokens: Arc::new(Mutex::new(HashSet::new())),
            sniped_tokens: Arc::new(Mutex::new(HashSet::new())),
            creator_tracked_tokens: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}

/// Run the dual-listener event loop. Reconnects automatically on drop.
pub async fn run(
    config: Arc<Config>,
    rpc_client: Arc<RpcClient>,
    bundle_wallets: Arc<Vec<(Keypair, f64)>>,
    ws_url: &str,
    state: &ListenerState,
    bot_active: Arc<AtomicBool>,
) {
    let enable_slidefun_create = matches!(config.snipe_mode.as_str(), "slidefun" | "both");
    let enable_raydium_migrate = matches!(config.snipe_mode.as_str(), "raydium" | "both" | "listen_creator");
    let enable_listen_creator = config.snipe_mode == "listen_creator";

    let slidefun_program = Pubkey::from_str(constants::slidefun_program()).unwrap();
    let amm_program = Pubkey::from_str(constants::RAYDIUM_AMM_PROGRAM).unwrap();

    log_info!("[WS] Connecting to WebSocket...");

    loop {
        match PubsubClient::new(ws_url).await {
            Ok(pubsub) => {
                // --- Subscribe to both programs ---
                let (mut stream_slidefun, _unsub_sf) = pubsub
                    .logs_subscribe(
                        RpcTransactionLogsFilter::Mentions(vec![slidefun_program.to_string()]),
                        RpcTransactionLogsConfig {
                            commitment: Some(CommitmentConfig::processed()),
                        },
                    )
                    .await
                    .unwrap();

                let (mut stream_raydium, _unsub_ray) = pubsub
                    .logs_subscribe(
                        RpcTransactionLogsFilter::Mentions(vec![amm_program.to_string()]),
                        RpcTransactionLogsConfig {
                            commitment: Some(CommitmentConfig::processed()),
                        },
                    )
                    .await
                    .unwrap();

                log_info!("[OK] ✅ Dual-listener active:");
                log_info!("   [A] Slide.fun : {}", constants::slidefun_program());
                log_info!("   [B] Raydium V4: {}", constants::RAYDIUM_AMM_PROGRAM);
                log_info!(
                    "   [C] Slidefun-create snipe: {}",
                    if enable_slidefun_create { "ACTIVE" } else { "DISABLED" }
                );
                log_info!("[OK] Waiting for events...\n");

                let mut last_heartbeat = std::time::Instant::now();

                loop {
                    // Heartbeat every 60 s
                    if last_heartbeat.elapsed().as_secs() >= 60 {
                        let count = state.graduating_tokens.lock().await.len();
                        log_info!("[HEARTBEAT] Running... Graduation watch-list: {} tokens", count);
                        last_heartbeat = std::time::Instant::now();
                    }

                    let event = tokio::select! {
                        result = stream_slidefun.next() => result.map(|log| ("slidefun", log)),
                        result = stream_raydium.next()  => result.map(|log| ("raydium",  log)),
                    };

                    match event {
                        Some(_) if !bot_active.load(Ordering::Relaxed) => {
                            // Bot is stopped, ignore all events
                            continue;
                        }
                        // ── LISTENER A: Slide.fun ─────────────────────────────
                        Some(("slidefun", log)) => {
                            let logs = log.value.logs.clone();

                            // Mode: SLIDEFUN_CREATE or LISTEN_CREATOR
                            if (enable_slidefun_create || enable_listen_creator) && slidefun_snipe::is_creation_signal(&logs) {
                                let signature = log.value.signature.clone();
                                log_info!("[SFSNIPE] 🆕 New Slide.fun token! TX: {}", signature);

                                let rpc_c = rpc_client.clone();
                                let cfg_c = config.clone();
                                let sniped_c = state.sniped_tokens.clone();
                                let wallets_c = bundle_wallets.clone();
                                let creator_c = state.creator_tracked_tokens.clone();
                                let grad_c = state.graduating_tokens.clone();

                                tokio::spawn(async move {
                                    if let Some((mint, creator)) =
                                        slidefun_snipe::extract_new_token_and_creator(&rpc_c, &signature).await
                                    {
                                        if enable_listen_creator {
                                            if cfg_c.app.target_creators.contains(&creator) {
                                                log_info!("[LISTEN_CREATOR] 🎯 Tracking token {} from creator {}", mint, creator);
                                                creator_c.lock().await.insert(mint.clone());
                                                // Also add to graduation watch-list so we can pre-create ATA if needed
                                                grad_c.lock().await.insert(mint.clone());
                                            } else {
                                                log_info!("   [SKIP] Creator {} not in target_creators", creator);
                                            }
                                            return; // Stop here, we don't buy immediately
                                        }

                                        if !cfg_c.is_whitelisted(&mint) {
                                            log_info!("   [SKIP] {} not in target whitelist", mint);
                                            return;
                                        }

                                        // Dedup
                                        {
                                            let mut s = sniped_c.lock().await;
                                            let key = format!("slidefun:{}", mint);
                                            if s.contains(&key) {
                                                log_info!("   [SKIP] Already sniped: {}", mint);
                                                return;
                                            }
                                            s.insert(key);
                                        }

                                        slidefun_snipe::handle_slidefun_buy(
                                            &cfg_c, rpc_c.clone(), &mint,
                                        )
                                        .await;

                                        // Bundle buy
                                        if !wallets_c.is_empty() && !cfg_c.dry_run {
                                            if let Some(fee_to) =
                                                slidefun_snipe::fetch_fee_to(&rpc_c).await
                                            {
                                                let bh = get_blockhash();
                                                bundle_buy::slidefun_bundle_buy(
                                                    &cfg_c,
                                                    &wallets_c,
                                                    &mint,
                                                    &fee_to,
                                                    bh,
                                                )
                                                .await;
                                            }
                                        }
                                    }
                                });
                            }

                            // Mode: RAYDIUM / BOTH — detect graduate migrate step
                            if enable_raydium_migrate && graduation::is_graduation_signal(&logs) {
                                let signature = log.value.signature.clone();
                                log_info!(
                                    "[SLIDE-FUN] 🎓 Graduation detected! TX: {}",
                                    signature
                                );

                                let rpc_c = rpc_client.clone();
                                let grad_c = state.graduating_tokens.clone();
                                let ata_c = state.ata_pre_created_tokens.clone();
                                let keypair_bytes = config.keypair.to_bytes();

                                tokio::spawn(async move {
                                    let keypair =
                                        Keypair::try_from(keypair_bytes.as_ref()).unwrap();
                                    if let Some(mint) = graduation::extract_graduating_token(
                                        &rpc_c, &signature, &logs,
                                    )
                                    .await
                                    {
                                        {
                                            let mut t = grad_c.lock().await;
                                            t.insert(mint.clone());
                                            log_info!(
                                                "[GRAD] ✅ Added {} to watch-list ({} total)",
                                                mint,
                                                t.len()
                                            );
                                        }

                                        // Pre-create ATA before the pool exists
                                        graduation::pre_create_ata(&rpc_c, &keypair, &mint).await;
                                        ata_c.lock().await.insert(mint);
                                    }
                                });
                            }
                        }

                        // ── LISTENER B: Raydium AMM V4 ───────────────────────
                        Some(("raydium", log)) => {
                            let logs_str = log.value.logs.join(" ");
                            let is_init = logs_str
                                .contains("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8 invoke")
                                && (logs_str.contains("initialize2:")
                                    || logs_str.contains("Initialize2"));

                            if !is_init {
                                continue;
                            }

                            let signature = log.value.signature.clone();
                            log_info!("[RAYDIUM] New AMM V4 pool detected: {}", signature);

                            let rpc_c = rpc_client.clone();
                            let cfg_c = config.clone();
                            let grad_c = state.graduating_tokens.clone();
                            let ata_c = state.ata_pre_created_tokens.clone();
                            let sniped_c = state.sniped_tokens.clone();
                            let wallets_c = bundle_wallets.clone();
                            let creator_c = state.creator_tracked_tokens.clone();

                            tokio::spawn(async move {
                                if let Some(pool_info) =
                                    get_pool_info(&rpc_c, &signature).await
                                {
                                    let token_key = pool_info.base_mint.to_string();

                                    let mut is_target = cfg_c.is_whitelisted(&token_key);
                                    if !is_target && cfg_c.snipe_mode == "listen_creator" {
                                        is_target = creator_c.lock().await.contains(&token_key);
                                    }
                                    if !is_target {
                                        log_info!("   [SKIP] {} not in target whitelist or tracked by creator", token_key);
                                        return;
                                    }

                                    // Skip if not from Slide.fun graduation (unless TEST_MODE)
                                    if !cfg_c.test_mode {
                                        let is_slidefun = {
                                            grad_c.lock().await.contains(&token_key)
                                        };
                                        if !is_slidefun {
                                            log_info!(
                                                "   [SKIP] {} not from Slide.fun",
                                                token_key
                                            );
                                            return;
                                        }
                                    } else {
                                        log_info!(
                                            "   [TEST_MODE] Bypassing Slide.fun check: {}",
                                            token_key
                                        );
                                    }

                                    // Dedup
                                    {
                                        let mut s = sniped_c.lock().await;
                                        let key = format!("raydium:{}", token_key);
                                        if s.contains(&key) {
                                            log_info!(
                                                "   [SKIP] Already sniped: {}",
                                                token_key
                                            );
                                            return;
                                        }
                                        s.insert(key);
                                    }

                                    let ata_pre = ata_c.lock().await.contains(&token_key);

                                    // Remove from graduation watch-list
                                    grad_c.lock().await.remove(&token_key);

                                    log_info!(
                                        "[SNIPE] 🚀 SLIDE-FUN TOKEN ON RAYDIUM! Sniping: {} (ATA pre-created: {})",
                                        token_key,
                                        ata_pre
                                    );

                                    // Main wallet buy
                                    handle_buy(&cfg_c, rpc_c.clone(), pool_info.clone(), ata_pre)
                                        .await;

                                    // Bundle buy (sub-wallets)
                                    if !wallets_c.is_empty() && !cfg_c.dry_run {
                                        let bh = get_blockhash();
                                        let pool_arc = Arc::new(pool_info);
                                        bundle_buy::raydium_bundle_buy(
                                            &cfg_c,
                                            &wallets_c,
                                            pool_arc,
                                            bh,
                                        )
                                        .await;
                                    }
                                }
                            });
                        }

                        Some(_) => {}

                        None => {
                            log_info!("[WARN] WebSocket stream ended — reconnecting...");
                            break;
                        }
                    }
                }
            }

            Err(e) => {
                log_info!("[ERROR] WebSocket connection failed: {} — retrying in 100ms...", e);
                sleep(Duration::from_millis(100)).await;
            }
        }
    }
}
