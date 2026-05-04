// ==================== MULTI-WALLET BUNDLE BUY ====================
//
// This module enables buying tokens simultaneously with multiple wallets
// by packing buy transactions into Jito bundles and firing them in parallel.
//
// Architecture:
//   1. Load N "sub-wallets" from wallets.json
//   2. For each sub-wallet: build a standalone buy TX (SOL → Token)
//   3. Pack TXs into Jito bundles (max 5 TXs per bundle)
//      - First TX of each bundle = Jito tip TX (from main wallet)
//      - Remaining 4 TXs = buy TXs from sub-wallets
//   4. Fire each bundle to exactly ONE Jito Block Engine endpoint (see JITO_BUNDLE_ENDPOINT_INDEX)
//
// wallets.json format:
//   [
//     {"private_key": "base58_private_key_1"},
//     {"private_key": "base58_private_key_2"},
//     ...
//   ]
//
// Jito bundle limits:
//   - Max 5 TXs per bundle
//   - First TX in each bundle pays the Jito tip
//   - SO: 1 tip TX + 4 buy TXs = 5 TXs per bundle
//   - For N wallets → ceil(N / 4) bundles

use crate::{constants, log_info};
use crate::config::Config;
use crate::transaction::send_bundle_to_url;
use crate::types::PoolInfo;
use bincode;
use bs58;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{v0::Message, VersionedMessage},
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signature::Signature,
    signer::{keypair::Keypair, Signer},
    system_instruction,
    transaction::VersionedTransaction,
};
use spl_associated_token_account::{
    get_associated_token_address,
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use std::str::FromStr;
use std::sync::Arc;

// ============================================
// Wallet loading
// ============================================

/// Load extra sub-wallets from a JSON file.
/// Returns a list of Keypairs to use for bundle buying.
pub fn load_wallets(path: &str) -> Vec<Keypair> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            log_info!("[BUNDLE] Could not read wallets file '{}': {}", path, e);
            return vec![];
        }
    };

    let entries: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            log_info!("[BUNDLE] Failed to parse wallets.json: {}", e);
            return vec![];
        }
    };

    let mut keypairs = Vec::new();
    if let Some(arr) = entries.as_array() {
        for (i, entry) in arr.iter().enumerate() {
            if let Some(pk_str) = entry["private_key"].as_str() {
                let kp = Keypair::from_base58_string(pk_str);
                log_info!("[BUNDLE] Loaded wallet[{}]: {}", i, kp.pubkey());
                keypairs.push(kp);
            }
        }
    }
    keypairs
}

// ============================================
// Transaction builders
// ============================================

/// Build a "Jito tip" transaction from the main wallet.
/// This must be the FIRST TX in every bundle.
/// `bundle_index` is added as extra lamports to make each tip TX unique
/// (same keypair + blockhash + amount = identical signature → Jito drops duplicates).
fn build_tip_tx(
    main_keypair: &Keypair,
    jito_tip_lamports: u64,
    bundle_index: usize,
    blockhash: Hash,
) -> Option<VersionedTransaction> {
    let user = main_keypair.pubkey();
    let jito_tip_address = Pubkey::from_str(constants::JITO_TIP_ADDRESS).unwrap();

    // Add bundle_index lamports so each tip TX has a unique signature
    let tip = jito_tip_lamports + bundle_index as u64;

    let ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(3_000),
        system_instruction::transfer(&user, &jito_tip_address, tip),
    ];

    match Message::try_compile(&user, &ixs, &[], blockhash) {
        Ok(msg) => {
            match VersionedTransaction::try_new(VersionedMessage::V0(msg), &[main_keypair]) {
                Ok(tx) => Some(tx),
                Err(e) => { log_info!("[BUNDLE] Tip TX sign error: {}", e); None }
            }
        }
        Err(e) => { log_info!("[BUNDLE] Tip TX compile error: {}", e); None }
    }
}

/// Build a single **Raydium AMM V4 buy** transaction for a sub-wallet.
pub fn build_raydium_buy_tx_for_wallet(
    keypair: &Keypair,
    pool_info: &PoolInfo,
    sol_lamports: u64,
    min_token_out: u64,
    cu_limit: u32,
    priority_fee: u64,
    blockhash: Hash,
) -> Option<VersionedTransaction> {
    use crate::transaction::build_swap_instruction;

    let user = keypair.pubkey();
    let wsol_mint = Pubkey::from_str(constants::WSOL_MINT).unwrap();
    let wsol_token_program = Pubkey::from_str(constants::TOKEN_PROGRAM).unwrap();
    let base_token_program = pool_info.base_token_program;
    let user_wsol_ata = get_associated_token_address(&user, &wsol_mint);
    let user_token_ata = get_associated_token_address_with_program_id(
        &user,
        &pool_info.base_mint,
        &base_token_program,
    );

    let swap_ix = build_swap_instruction(
        pool_info,
        user,
        user_token_ata,
        user_wsol_ata,
        sol_lamports,
        min_token_out,
        base_token_program,
    );

    let ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(cu_limit),
        ComputeBudgetInstruction::set_compute_unit_price(priority_fee),
        create_associated_token_account_idempotent(&user, &user, &wsol_mint, &wsol_token_program),
        system_instruction::transfer(&user, &user_wsol_ata, sol_lamports),
        Instruction {
            program_id: wsol_token_program,
            accounts: vec![AccountMeta::new(user_wsol_ata, false)],
            data: vec![17], // SyncNative
        },
        create_associated_token_account_idempotent(
            &user,
            &user,
            &pool_info.base_mint,
            &base_token_program,
        ),
        swap_ix,
    ];

    match Message::try_compile(&user, &ixs, &[], blockhash) {
        Ok(msg) => {
            match VersionedTransaction::try_new(VersionedMessage::V0(msg), &[keypair]) {
                Ok(tx) => {
                    let bytes = bincode::serialize(&tx).unwrap_or_default();
                    if bytes.len() > 1232 {
                        log_info!("[BUNDLE] Wallet {} TX too large: {} bytes", user, bytes.len());
                        return None;
                    }
                    Some(tx)
                }
                Err(e) => { log_info!("[BUNDLE] Wallet {} sign error: {}", user, e); None }
            }
        }
        Err(e) => { log_info!("[BUNDLE] Wallet {} compile error: {}", user, e); None }
    }
}

/// Build a single **Slide.fun bonding curve buy** transaction for a sub-wallet.
pub fn build_slidefun_buy_tx_for_wallet(
    keypair: &Keypair,
    token_mint: &Pubkey,
    fee_to: &Pubkey,
    sol_lamports: u64,
    cu_limit: u32,
    priority_fee: u64,
    blockhash: Hash,
) -> Option<VersionedTransaction> {
    use crate::slidefun_snipe::build_slidefun_buy_instruction;

    let user = keypair.pubkey();
    let wsol_mint = Pubkey::from_str(constants::WSOL_MINT).unwrap();
    let token_program = Pubkey::from_str(constants::TOKEN_PROGRAM).unwrap();
    let user_payment_ata = get_associated_token_address(&user, &wsol_mint);

    let buy_ix = build_slidefun_buy_instruction(
        &user,
        token_mint,
        &wsol_mint,
        fee_to,
        sol_lamports,
        0, // no min tokens out (max speed)
    );

    let ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(cu_limit),
        ComputeBudgetInstruction::set_compute_unit_price(priority_fee),
        create_associated_token_account_idempotent(&user, &user, &wsol_mint, &token_program),
        system_instruction::transfer(&user, &user_payment_ata, sol_lamports),
        Instruction {
            program_id: token_program,
            accounts: vec![AccountMeta::new(user_payment_ata, false)],
            data: vec![17], // SyncNative
        },
        create_associated_token_account_idempotent(&user, &user, token_mint, &token_program),
        buy_ix,
    ];

    match Message::try_compile(&user, &ixs, &[], blockhash) {
        Ok(msg) => {
            match VersionedTransaction::try_new(VersionedMessage::V0(msg), &[keypair]) {
                Ok(tx) => {
                    let bytes = bincode::serialize(&tx).unwrap_or_default();
                    if bytes.len() > 1232 {
                        log_info!("[BUNDLE] Wallet {} SF TX too large: {} bytes", user, bytes.len());
                        return None;
                    }
                    Some(tx)
                }
                Err(e) => { log_info!("[BUNDLE] Wallet {} SF sign error: {}", user, e); None }
            }
        }
        Err(e) => { log_info!("[BUNDLE] Wallet {} SF compile error: {}", user, e); None }
    }
}

// ============================================
// Bundle packing + sending
// ============================================

/// Encode a VersionedTransaction to base58 (Jito bundle format).
fn encode_tx(tx: &VersionedTransaction) -> Option<String> {
    let bytes = bincode::serialize(tx).ok()?;
    Some(bs58::encode(&bytes).into_string())
}

fn sig_short(sig: &Signature) -> String {
    let t = sig.to_string();
    if t.len() > 16 {
        format!("{}…", &t[..16])
    } else {
        t
    }
}

/// Log Solscan URLs for tip + each buy TX (same construction as `pack_into_bundles`).
fn log_bundle_solscan_links(
    buy_txs: &[VersionedTransaction],
    pending_wallet_indices: &[usize],
    main_keypair: &Keypair,
    jito_tip_lamports: u64,
    blockhash: Hash,
) {
    let n_bundles = (buy_txs.len() + 3) / 4;
    log_info!("[BUNDLE] TX preview (Solscan) — same sigs as in Jito bundle:");
    for chunk_i in 0..n_bundles {
        if let Some(tip) = build_tip_tx(main_keypair, jito_tip_lamports, chunk_i, blockhash) {
            if let Some(sig) = tip.signatures.first() {
                log_info!(
                    "[BUNDLE]   tip bundle[{}]: https://solscan.io/tx/{}",
                    chunk_i,
                    sig
                );
            }
        }
    }
    for (i, tx) in buy_txs.iter().enumerate() {
        if let Some(sig) = tx.signatures.first() {
            let wi = pending_wallet_indices.get(i).copied().unwrap_or(i);
            log_info!(
                "[BUNDLE]   buy wallet[{}]: https://solscan.io/tx/{}",
                wi,
                sig
            );
        }
    }
}

/// After submit: whether each signature landed and succeeded (`searchTransactionHistory = true`).
async fn log_signature_onchain_status(rpc: &RpcClient, label: &str, sig: &Signature) {
    match rpc
        .get_signature_status_with_commitment_and_history(sig, CommitmentConfig::confirmed(), true)
        .await
    {
        Ok(Some(Ok(()))) => log_info!(
            "[BUNDLE]   {} {} → ✅ confirmed",
            label,
            sig_short(sig)
        ),
        Ok(Some(Err(e))) => log_info!(
            "[BUNDLE]   {} {} → ❌ failed: {:?}",
            label,
            sig_short(sig),
            e
        ),
        Ok(None) => log_info!(
            "[BUNDLE]   {} {} → ⏳ not in ledger (dropped / expired / bundle not included)",
            label,
            sig_short(sig)
        ),
        Err(e) => log_info!("[BUNDLE]   {} {} → RPC error: {}", label, sig_short(sig), e),
    }
}

async fn log_bundle_onchain_after_wait(
    rpc: &RpcClient,
    buy_txs: &[VersionedTransaction],
    pending_wallet_indices: &[usize],
    main_keypair: &Keypair,
    jito_tip_lamports: u64,
    blockhash: Hash,
) {
    log_info!("[BUNDLE] On-chain status (getSignatureStatuses + history):");
    let n_bundles = (buy_txs.len() + 3) / 4;
    for chunk_i in 0..n_bundles {
        if let Some(tip) = build_tip_tx(main_keypair, jito_tip_lamports, chunk_i, blockhash) {
            if let Some(sig) = tip.signatures.first() {
                log_signature_onchain_status(rpc, &format!("tip bundle[{}]", chunk_i), sig).await;
            }
        }
    }
    for (i, tx) in buy_txs.iter().enumerate() {
        if let Some(sig) = tx.signatures.first() {
            let wi = pending_wallet_indices.get(i).copied().unwrap_or(i);
            log_signature_onchain_status(rpc, &format!("buy wallet[{}]", wi), sig).await;
        }
    }
}

/// Broadcast each bundle to **all** Jito Block Engine endpoints in parallel.
/// → If any endpoint is rate-limited, the others will still accept the bundle.
/// → First acceptance wins; subsequent acceptances are silently ignored.
async fn fire_bundles(bundles: Vec<Vec<String>>) {
    use constants::{JITO_BUNDLE_URLS, JITO_REGIONS};

    for (i, bundle) in bundles.into_iter().enumerate() {
        // Fire to all endpoints in parallel
        let futs: Vec<_> = JITO_BUNDLE_URLS
            .iter()
            .zip(JITO_REGIONS.iter())
            .map(|(&url, &region)| {
                let b = bundle.clone();
                async move {
                    let result = send_bundle_to_url(&b, url).await;
                    (region, result)
                }
            })
            .collect();

        let results = futures::future::join_all(futs).await;

        let mut accepted = false;
        for (region, result) in &results {
            match result {
                Ok(id) => {
                    if !accepted {
                        log_info!(
                            "[BUNDLE] Bundle[{}] → {} ✅ {}",
                            i,
                            region,
                            &id[..id.len().min(20)]
                        );
                        log_info!(
                            "[BUNDLE]   Jito bundle id: {} — https://explorer.jito.wtf/bundle-watch",
                            id
                        );
                        accepted = true;
                    }
                    // else: other endpoints also accepted — silent, avoid log spam
                }
                Err(e) => log_info!("[BUNDLE] Bundle[{}] → {} ❌ {}", i, region, e),
            }
        }
        if !accepted {
            log_info!("[BUNDLE] Bundle[{}] → ❌ ALL endpoints rejected/rate-limited", i);
        }
    }
}

// ============================================
// Public entry points
// ============================================

/// Helper: pack a list of buy TXs into Jito bundles (tip TX + up to 4 buy TXs each).
fn pack_into_bundles(
    buy_txs: &[VersionedTransaction],
    main_keypair: &solana_sdk::signer::keypair::Keypair,
    jito_tip_lamports: u64,
    blockhash: Hash,
) -> Vec<Vec<String>> {
    let mut bundles: Vec<Vec<String>> = Vec::new();
    for chunk in buy_txs.chunks(4) {
        let bundle_index = bundles.len();
        let tip_tx = match build_tip_tx(main_keypair, jito_tip_lamports, bundle_index, blockhash) {
            Some(tx) => tx,
            None => { log_info!("[BUNDLE] Could not build tip TX for bundle {}", bundle_index); continue; }
        };
        let mut bundle: Vec<String> = Vec::new();
        if let Some(encoded) = encode_tx(&tip_tx) { bundle.push(encoded); }
        for buy_tx in chunk {
            if let Some(encoded) = encode_tx(buy_tx) { bundle.push(encoded); }
        }
        log_info!("[BUNDLE]   Bundle[{}] → {} TXs", bundles.len(), bundle.len());
        bundles.push(bundle);
    }
    bundles
}

/// Fire a multi-wallet bundle buy on Raydium AMM V4.
///
/// Strategy:
///   1. Build all buy TXs and record initial wallet balances.
///   2. Fire all bundles (each routed to a dedicated Jito region).
///   3. Wait CONFIRM_WAIT_SECS for bundles to land.
///   4. Check which wallets actually bought (balance decreased ≥ sol/2).
///   5. Retry up to MAX_RETRIES times for wallets that didn't buy.
pub async fn raydium_bundle_buy(
    config: &Config,
    wallets: &[Keypair],
    pool_info: Arc<PoolInfo>,
    sol_per_wallet: f64,
    blockhash: Hash,
) {
    if wallets.is_empty() {
        log_info!("[BUNDLE] No bundle wallets loaded — skipping bundle buy");
        return;
    }

    const MAX_RETRIES: usize = 2;       // up to 3 total attempts (1 initial + 2 retries)
    const CONFIRM_WAIT_SECS: u64 = 6;   // seconds to wait before checking if bundles landed

    let sol_lamports = (sol_per_wallet * LAMPORTS_PER_SOL as f64) as u64;
    let jito_tip_lamports = (config.jito_tip * LAMPORTS_PER_SOL as f64) as u64;
    // Threshold: if balance dropped by ≥ half the swap amount the wallet bought
    let bought_threshold = sol_lamports / 2;

    log_info!("[BUNDLE] 🚀 Raydium bundle buy: {} wallets × {} SOL each (tip={} SOL each bundle)",
        wallets.len(), sol_per_wallet, config.jito_tip);

    // RPC client for balance checks / fresh blockhash on retry
    let rpc_url = format!("https://mainnet.helius-rpc.com/?api-key={}", config.helius_api_key);
    let rpc = RpcClient::new(rpc_url);

    // Capture pre-buy balances for all wallets (used to detect confirmed buys)
    let wallet_pubkeys: Vec<Pubkey> = wallets.iter().map(|kp| kp.pubkey()).collect();
    let mut pre_balances: Vec<u64> = Vec::with_capacity(wallets.len());
    {
        let futs: Vec<_> = wallet_pubkeys.iter().map(|pk| rpc.get_balance(pk)).collect();
        let results = futures::future::join_all(futs).await;
        for (i, res) in results.into_iter().enumerate() {
            let bal = res.unwrap_or(0);
            pre_balances.push(bal);
            log_info!("[BUNDLE]   Wallet[{}] pre-balance: {:.6} SOL", i, bal as f64 / 1e9);
        }
    }

    // Track which wallets have confirmed their buy
    let mut wallet_bought: Vec<bool> = vec![false; wallets.len()];

    // -- Attempt loop (initial fire + retries) --
    let mut current_bh = blockhash;
    for attempt in 0..=MAX_RETRIES {
        // Find wallets that still need to buy
        let pending: Vec<usize> = (0..wallets.len())
            .filter(|&i| !wallet_bought[i])
            .collect();

        if pending.is_empty() {
            log_info!("[BUNDLE] 🎉 All {} wallets confirmed bought!", wallets.len());
            break;
        }
        if attempt > 0 {
            log_info!("[BUNDLE] ⟳ Retry {}/{}: {} wallet(s) pending — fetching fresh blockhash...",
                attempt, MAX_RETRIES, pending.len());
            current_bh = match rpc.get_latest_blockhash().await {
                Ok(bh) => bh,
                Err(e) => { log_info!("[BUNDLE] ❌ Failed to get fresh blockhash: {}", e); break; }
            };
        }
        // Build buy TXs for pending wallets only
        let mut buy_txs: Vec<VersionedTransaction> = Vec::new();
        for &wi in &pending {
            let keypair = &wallets[wi];
            match build_raydium_buy_tx_for_wallet(
                keypair, &pool_info, sol_lamports, 0,
                config.cu_limit, config.priority_fee, current_bh,
            ) {
                Some(tx) => {
                    let bytes = bincode::serialize(&tx).unwrap_or_default();
                    log_info!("[BUNDLE]   Wallet[{}] {} → {} bytes",
                        wi, keypair.pubkey(), bytes.len());
                    buy_txs.push(tx);
                }
                None => log_info!("[BUNDLE]   Wallet[{}] failed to build TX", wi),
            }
        }

        if buy_txs.is_empty() {
            log_info!("[BUNDLE] No valid buy TXs — aborting");
            break;
        }

        let bundles = pack_into_bundles(&buy_txs, &config.keypair, jito_tip_lamports, current_bh);
        log_bundle_solscan_links(
            &buy_txs,
            &pending,
            &config.keypair,
            jito_tip_lamports,
            current_bh,
        );
        log_info!("[BUNDLE] Firing {} bundle(s) (attempt {}/{})...",
            bundles.len(), attempt + 1, MAX_RETRIES + 1);
        fire_bundles(bundles).await;

        // Wait for bundles to land on-chain before checking balances
        log_info!("[BUNDLE] Waiting {}s for bundles to land...", CONFIRM_WAIT_SECS);
        tokio::time::sleep(tokio::time::Duration::from_secs(CONFIRM_WAIT_SECS)).await;

        log_bundle_onchain_after_wait(
            &rpc,
            &buy_txs,
            &pending,
            &config.keypair,
            jito_tip_lamports,
            current_bh,
        )
        .await;

        // Update bought status based on current balances
        let pending_pks: Vec<Pubkey> = pending.iter().map(|&wi| wallet_pubkeys[wi]).collect();
        let futs: Vec<_> = pending_pks.iter().map(|pk| rpc.get_balance(pk)).collect();
        let results = futures::future::join_all(futs).await;
        for (idx, res) in results.into_iter().enumerate() {
            let wi = pending[idx];
            let cur_bal = res.unwrap_or(pre_balances[wi]);
            let spent = pre_balances[wi].saturating_sub(cur_bal);
            if spent >= bought_threshold {
                wallet_bought[wi] = true;
                log_info!("[BUNDLE] ✅ Wallet[{}] confirmed bought (spent {} lamports)", wi, spent);
            } else {
                log_info!("[BUNDLE] ⏳ Wallet[{}] not confirmed yet (spent {} lamports, need ≥{})",
                    wi, spent, bought_threshold);
            }
        }

        if attempt == MAX_RETRIES {
            let unbought: Vec<usize> = (0..wallets.len()).filter(|&i| !wallet_bought[i]).collect();
            if !unbought.is_empty() {
                log_info!("[BUNDLE] ⚠️  {} wallet(s) still unconfirmed after {} attempts: {:?}",
                    unbought.len(), MAX_RETRIES + 1, unbought);
            } else {
                log_info!("[BUNDLE] 🎉 All {} wallets confirmed!", wallets.len());
            }
        }
    }

    log_info!("[BUNDLE] ✅ Done");
}

/// Fire a multi-wallet bundle buy on Slide.fun Bonding Curve.
/// Same retry strategy as `raydium_bundle_buy`.
pub async fn slidefun_bundle_buy(
    config: &Config,
    wallets: &[Keypair],
    token_mint: &str,
    fee_to: &Pubkey,
    sol_per_wallet: f64,
    blockhash: Hash,
) {
    if wallets.is_empty() {
        log_info!("[BUNDLE] No bundle wallets loaded — skipping bundle buy");
        return;
    }

    let token_mint_pk = match Pubkey::from_str(token_mint) {
        Ok(pk) => pk,
        Err(e) => { log_info!("[BUNDLE] Invalid token mint: {}", e); return; }
    };

    const MAX_RETRIES: usize = 2;
    const CONFIRM_WAIT_SECS: u64 = 6;

    let sol_lamports = (sol_per_wallet * LAMPORTS_PER_SOL as f64) as u64;
    let jito_tip_lamports = (config.jito_tip * LAMPORTS_PER_SOL as f64) as u64;
    let bought_threshold = sol_lamports / 2;

    log_info!("[BUNDLE] 🚀 Slide.fun bundle buy: {} wallets × {} SOL each (tip={} SOL each bundle)",
        wallets.len(), sol_per_wallet, config.jito_tip);

    let rpc_url = format!("https://mainnet.helius-rpc.com/?api-key={}", config.helius_api_key);
    let rpc = RpcClient::new(rpc_url);

    // Capture pre-buy balances
    let sf_wallet_pubkeys: Vec<Pubkey> = wallets.iter().map(|kp| kp.pubkey()).collect();
    let mut pre_balances: Vec<u64> = Vec::with_capacity(wallets.len());
    {
        let futs: Vec<_> = sf_wallet_pubkeys.iter().map(|pk| rpc.get_balance(pk)).collect();
        for (i, res) in futures::future::join_all(futs).await.into_iter().enumerate() {
            let bal = res.unwrap_or(0);
            pre_balances.push(bal);
            log_info!("[BUNDLE]   Wallet[{}] pre-balance: {:.6} SOL", i, bal as f64 / 1e9);
        }
    }

    let mut wallet_bought: Vec<bool> = vec![false; wallets.len()];
    let mut current_bh = blockhash;

    for attempt in 0..=MAX_RETRIES {
        let pending: Vec<usize> = (0..wallets.len()).filter(|&i| !wallet_bought[i]).collect();
        if pending.is_empty() {
            log_info!("[BUNDLE] 🎉 All {} wallets confirmed bought!", wallets.len());
            break;
        }

        if attempt > 0 {
            log_info!("[BUNDLE] ⟳ Retry {}/{}: {} wallet(s) pending...",
                attempt, MAX_RETRIES, pending.len());
            current_bh = match rpc.get_latest_blockhash().await {
                Ok(bh) => bh,
                Err(e) => { log_info!("[BUNDLE] ❌ Fresh blockhash failed: {}", e); break; }
            };
        }

        let mut buy_txs: Vec<VersionedTransaction> = Vec::new();
        for &wi in &pending {
            match build_slidefun_buy_tx_for_wallet(
                &wallets[wi], &token_mint_pk, fee_to, sol_lamports,
                config.cu_limit, config.priority_fee, current_bh,
            ) {
                Some(tx) => {
                    let bytes = bincode::serialize(&tx).unwrap_or_default();
                    log_info!("[BUNDLE]   Wallet[{}] {} → {} bytes",
                        wi, wallets[wi].pubkey(), bytes.len());
                    buy_txs.push(tx);
                }
                None => log_info!("[BUNDLE]   Wallet[{}] failed to build TX", wi),
            }
        }

        if buy_txs.is_empty() { break; }

        let bundles = pack_into_bundles(&buy_txs, &config.keypair, jito_tip_lamports, current_bh);
        log_bundle_solscan_links(
            &buy_txs,
            &pending,
            &config.keypair,
            jito_tip_lamports,
            current_bh,
        );
        log_info!("[BUNDLE] Firing {} bundle(s) (attempt {}/{})...",
            bundles.len(), attempt + 1, MAX_RETRIES + 1);
        fire_bundles(bundles).await;

        log_info!("[BUNDLE] Waiting {}s for bundles to land...", CONFIRM_WAIT_SECS);
        tokio::time::sleep(tokio::time::Duration::from_secs(CONFIRM_WAIT_SECS)).await;

        log_bundle_onchain_after_wait(
            &rpc,
            &buy_txs,
            &pending,
            &config.keypair,
            jito_tip_lamports,
            current_bh,
        )
        .await;

        let sf_pending_pks: Vec<Pubkey> = pending.iter().map(|&wi| sf_wallet_pubkeys[wi]).collect();
        let futs: Vec<_> = sf_pending_pks.iter().map(|pk| rpc.get_balance(pk)).collect();
        for (idx, res) in futures::future::join_all(futs).await.into_iter().enumerate() {
            let wi = pending[idx];
            let cur_bal = res.unwrap_or(pre_balances[wi]);
            let spent = pre_balances[wi].saturating_sub(cur_bal);
            if spent >= bought_threshold {
                wallet_bought[wi] = true;
                log_info!("[BUNDLE] ✅ Wallet[{}] confirmed bought (spent {} lamports)", wi, spent);
            } else {
                log_info!("[BUNDLE] ⏳ Wallet[{}] not confirmed yet (spent {} lamports)", wi, spent);
            }
        }

        if attempt == MAX_RETRIES {
            let unbought: Vec<usize> = (0..wallets.len()).filter(|&i| !wallet_bought[i]).collect();
            if !unbought.is_empty() {
                log_info!("[BUNDLE] ⚠️  {} wallet(s) unconfirmed after {} attempts: {:?}",
                    unbought.len(), MAX_RETRIES + 1, unbought);
            }
        }
    }

    log_info!("[BUNDLE] ✅ Done");
}
