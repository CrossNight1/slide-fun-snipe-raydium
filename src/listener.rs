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

use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use futures_util::StreamExt;
use solana_client::{
    nonblocking::{pubsub_client::PubsubClient, rpc_client::RpcClient},
    rpc_config::{RpcTransactionLogsConfig, RpcTransactionLogsFilter},
};
use solana_sdk::{commitment_config::CommitmentConfig, signer::keypair::Keypair};
use tokio::{
    sync::Mutex,
    time::{sleep, Duration},
};

use crate::{
    blockhash::get_blockhash, bundle_buy, config::Config, graduation, handler::handle_buy,
    log_info, pool::get_pool_info, slidefun_snipe,
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
    let enable_slidefun_create = matches!(
        config.snipe_mode.to_lowercase().as_str(),
        "slidefun" | "both" | "listen_creator"
    );
    let enable_raydium_migrate = matches!(
        config.snipe_mode.to_lowercase().as_str(),
        "raydium" | "both" | "listen_creator"
    );

    let slidefun_program = config.slidefun_program();
    let amm_program = config.raydium_program();
    let amm_program_str = amm_program.to_string();

    // Cache Slide.fun fee_to at startup
    crate::slidefun_snipe::pre_fetch_fee_to(&rpc_client, &slidefun_program).await;

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
                log_info!("   [A] Slide.fun : {}", slidefun_program);
                log_info!("   [B] Raydium V4: {}", amm_program);
                log_info!(
                    "   [C] Slidefun-create snipe: {}",
                    if enable_slidefun_create {
                        "ACTIVE"
                    } else {
                        "DISABLED"
                    }
                );
                log_info!(
                    "   [D] Listen-creator snipe : {}",
                    if config.app.listen_creator {
                        "ACTIVE"
                    } else {
                        "DISABLED"
                    }
                );
                log_info!("[OK] Waiting for events...\n");

                let mut last_heartbeat = std::time::Instant::now();

                loop {
                    // Heartbeat every 60 s
                    if last_heartbeat.elapsed().as_secs() >= 60 {
                        let count = state.graduating_tokens.lock().await.len();
                        log_info!(
                            "[HEARTBEAT] Running... Graduation watch-list: {} tokens",
                            count
                        );
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

                            // Mode: SLIDEFUN_CREATE or config.app.listen_creator
                            if (enable_slidefun_create || config.app.listen_creator)
                                && slidefun_snipe::is_creation_signal(
                                    &logs,
                                    &slidefun_program.to_string(),
                                )
                            {
                                let signature = log.value.signature.clone();
                                log_info!("[SFSNIPE] 🆕 New Slide.fun token! TX: {}", signature);

                                let rpc_c = rpc_client.clone();
                                let cfg_c = config.clone();
                                let sniped_c = state.sniped_tokens.clone();
                                let wallets_c = bundle_wallets.clone();
                                let creator_c = state.creator_tracked_tokens.clone();
                                let grad_c = state.graduating_tokens.clone();

                                tokio::spawn(async move {
                                    if let Some((mint, creator, token_program)) =
                                        slidefun_snipe::extract_new_token_and_creator(
                                            &rpc_c,
                                            &signature,
                                            &cfg_c.slidefun_program(),
                                        )
                                        .await
                                    {
                                        let mut should_buy = false;

                                        // 1. Check Creator Tracking
                                        let is_creator_mode =
                                            cfg_c.snipe_mode.to_lowercase() == "listen_creator";
                                        if cfg_c.app.listen_creator || is_creator_mode {
                                            if cfg_c.is_creator_tracked(&creator) {
                                                log_info!("[LISTEN_CREATOR] 🎯 Match! Tracking token {} from creator {}", mint, creator);
                                                creator_c.lock().await.insert(mint.clone());
                                                grad_c.lock().await.insert(mint.clone());
                                                should_buy = true;
                                            }
                                        }

                                        // 2. Check General Slide.fun Snipe (ONLY if mode is Slidefun/Both)
                                        let smode = cfg_c.snipe_mode.to_lowercase();
                                        if !should_buy && (smode == "slidefun" || smode == "both") {
                                            if cfg_c.is_whitelisted(&mint) {
                                                log_info!(
                                                    "[SFSNIPE] 🎯 Match! Sniping {}...",
                                                    mint
                                                );
                                                should_buy = true;
                                            }
                                        }

                                        if !should_buy {
                                            log_info!("   [SKIP] {} does not match target creators or whitelist", mint);
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

                                        // Main buy
                                        let rpc_m = rpc_c.clone();
                                        let cfg_m = cfg_c.clone();
                                        let mint_m = mint.clone();
                                        tokio::spawn(async move {
                                            slidefun_snipe::handle_slidefun_buy(
                                                &cfg_m,
                                                rpc_m,
                                                &mint_m,
                                                token_program,
                                            )
                                            .await;
                                        });

                                        // Bundle buy
                                        if !wallets_c.is_empty() && !cfg_c.dry_run {
                                            let cfg_b = cfg_c.clone();
                                            let mint_b = mint.clone();
                                            let wallets_b = wallets_c.clone();
                                            tokio::spawn(async move {
                                                // Use cached fee_to to avoid 600ms RPC delay
                                                if let Some(fee_to) = slidefun_snipe::get_cached_fee_to() {
                                                    let bh = get_blockhash();
                                                    bundle_buy::slidefun_bundle_buy(
                                                        &cfg_b,
                                                        &wallets_b,
                                                        &mint_b,
                                                        token_program,
                                                        &fee_to,
                                                        bh,
                                                    )
                                                    .await;
                                                }
                                            });
                                        }
                                    }
                                });
                            }

                            // Mode: RAYDIUM / BOTH — detect graduate migrate step
                            if enable_raydium_migrate
                                && graduation::is_graduation_signal(
                                    &logs,
                                    &slidefun_program.to_string(),
                                )
                            {
                                let signature = log.value.signature.clone();
                                log_info!("[SLIDE-FUN] 🎓 Graduation detected! TX: {}", signature);

                                let rpc_c = rpc_client.clone();
                                let grad_c = state.graduating_tokens.clone();
                                let ata_c = state.ata_pre_created_tokens.clone();
                                let keypair_bytes = config.keypair.to_bytes();

                                tokio::spawn(async move {
                                    let keypair =
                                        Keypair::try_from(keypair_bytes.as_ref()).unwrap();
                                    if let Some(mint) = graduation::extract_graduating_token(
                                        &rpc_c,
                                        &signature,
                                        &logs,
                                        &slidefun_program,
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
                            let is_init = logs_str.contains(&format!("{} invoke", amm_program_str))
                                && (logs_str.contains("initialize2:")
                                    || logs_str.contains("Initialize2"));

                            if !is_init {
                                continue;
                            }

                            let signature = log.value.signature.clone();
                            log_info!("[RAYDIUM] New AMM V4 pool detected: {}", signature);

                            let rpc_c = rpc_client.clone();
                            let cfg_c = config.clone();
                            let ata_c = state.ata_pre_created_tokens.clone();
                            let sniped_c = state.sniped_tokens.clone();
                            let wallets_c = bundle_wallets.clone();

                            tokio::spawn(async move {
                                log_info!("[RAYDIUM] 🔍 Fetching pool info for TX...");
                                match get_pool_info(&rpc_c, &signature, cfg_c.raydium_program()).await {
                                    Some(pool_info) => {
                                        let token_key = pool_info.base_mint.to_string();

                                        // Raydium listener: snipe ALL new pools.
                                        // Creator tracking only applies to Slide.fun listings.
                                        // Dedup only — avoid double-buying the same pool.
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

                                        log_info!(
                                        "[SNIPE] 🚀 New Raydium pool! Sniping: {} (ATA pre-created: {})",
                                        token_key,
                                        ata_pre
                                    );

                                        // Main wallet buy
                                        let cfg_m = cfg_c.clone();
                                        let rpc_m = rpc_c.clone();
                                        let pool_m = pool_info.clone();
                                        tokio::spawn(async move {
                                            handle_buy(
                                                &cfg_m,
                                                rpc_m,
                                                pool_m,
                                                ata_pre,
                                            )
                                            .await;
                                        });

                                        // Bundle buy (sub-wallets)
                                        if !wallets_c.is_empty() && !cfg_c.dry_run {
                                            let cfg_b = cfg_c.clone();
                                            let wallets_b = wallets_c.clone();
                                            tokio::spawn(async move {
                                                let bh = get_blockhash();
                                                let pool_arc = Arc::new(pool_info);
                                                bundle_buy::raydium_bundle_buy(
                                                    &cfg_b, &wallets_b, pool_arc, bh,
                                                )
                                                .await;
                                            });
                                        }
                                    } // end Some(pool_info)
                                    None => {
                                        log_info!("[RAYDIUM] ⚠️  Could not parse pool info from TX — skipping snipe");
                                        log_info!(
                                            "   TX: https://solscan.io/tx/{}?cluster=devnet",
                                            signature
                                        );
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
                log_info!(
                    "[ERROR] WebSocket connection failed: {} — retrying in 100ms...",
                    e
                );
                sleep(Duration::from_millis(100)).await;
            }
        }
    }
}
