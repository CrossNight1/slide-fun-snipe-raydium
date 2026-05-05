// ==================== HANDLER: Execute the snipe ====================
// When a Slide.fun graduated token's Raydium pool is detected,
// build and fire the swap transaction as fast as possible.

use crate::{
    blockhash::get_blockhash,
    config::Config,
    constants::{WSOL_MINT, JITO_TIP_ADDRESS},
    log_info,
    transaction::build_swap_instruction,
    types::PoolInfo,
};
use solana_client::{nonblocking::rpc_client::RpcClient, rpc_config::RpcSendTransactionConfig};
#[allow(deprecated)]
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    message::{v0::Message, VersionedMessage},
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signer::Signer,
    system_instruction,
    transaction::VersionedTransaction,
};
use spl_associated_token_account::{
    get_associated_token_address,
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use std::{str::FromStr, sync::Arc};

pub async fn handle_buy(config: &Config, rpc_client: Arc<RpcClient>, pool_info: PoolInfo, ata_pre_created: bool) {
    log_info!("\n[SNIPE] 🎯 SLIDE-FUN GRADUATED TOKEN DETECTED ON RAYDIUM!");
    log_info!("   AMM ID: {}", pool_info.amm_id);
    log_info!("   Token: {}", pool_info.base_mint);
    if pool_info.pool_sol_amount > 0 {
        log_info!("   Pool size: {:.4} SOL", pool_info.pool_sol_amount as f64 / 1e9);
    }

    let user = config.keypair.pubkey();
    let base_token_program = pool_info.base_token_program;
    let jito_tip_address = Pubkey::from_str(JITO_TIP_ADDRESS).unwrap();
    let wsol_mint = Pubkey::from_str(WSOL_MINT).unwrap();

    let sol_lamports = (config.sol_amount * LAMPORTS_PER_SOL as f64) as u64;
    let min_token_out = 1; // 100% slippage - buy at any price for speed

    let user_token_account = get_associated_token_address_with_program_id(
        &user,
        &pool_info.base_mint,
        &base_token_program,
    );
    let user_wsol_account = get_associated_token_address(&user, &wsol_mint);

    // === BUILD TX: Minimal instructions for maximum speed ===
    let mut instructions: Vec<Instruction> = vec![];

    // 1-2. Compute budget
    instructions.push(ComputeBudgetInstruction::set_compute_unit_limit(config.cu_limit));
    instructions.push(ComputeBudgetInstruction::set_compute_unit_price(config.priority_fee));

    // 3. Create ATA for new token (skip if already pre-created during graduation detection)
    if !ata_pre_created {
        instructions.push(create_associated_token_account_idempotent(
            &user,
            &user,
            &pool_info.base_mint,
            &base_token_program,
        ));
        log_info!("   [+] ATA creation included in TX");
    } else {
        log_info!("   [+] ATA pre-created ✅ - skipping instruction (4 ix instead of 5)");
    }

    // 4. Swap WSOL -> Token
    instructions.push(build_swap_instruction(
        &pool_info,
        user,
        user_wsol_account,
        user_token_account,
        sol_lamports,
        min_token_out,
        base_token_program,
    ));

    // 5. Jito tip for priority
    instructions.push(system_instruction::transfer(
        &user,
        &jito_tip_address,
        (config.jito_tip * LAMPORTS_PER_SOL as f64) as u64,
    ));

    // DRY RUN: show info only
    if config.dry_run {
        let blockhash = get_blockhash();
        let serialized = {
            let msg = Message::try_compile(&user, &instructions, &[], blockhash).unwrap();
            let tx = VersionedTransaction::try_new(VersionedMessage::V0(msg), &[&config.keypair]).unwrap();
            bincode::serialize(&tx).unwrap()
        };
        log_info!("\n[TEST] === DRY RUN MODE ===");
        log_info!("   [+] Transaction created successfully!");
        log_info!("   [+] Instructions: {}", instructions.len());
        log_info!("   [+] SOL amount: {} SOL", config.sol_amount);
        log_info!("   [+] Jito tip: {} SOL", config.jito_tip);
        log_info!("   [+] TX size: {} bytes", serialized.len());
        log_info!("   [WARN] NOT SENDING - DRY_RUN mode");
        log_info!("   [TIP] Set DRY_RUN=false in .env to trade for real\n");
        return;
    }

    // === LIVE MODE: SPAM for maximum chance ===
    // Slide.fun pools open immediately (open_time = 0), so we fire aggressively
    log_info!("[SNIPE] 🚀 FIRING TX - {} instructions, Jito tip: {} SOL",
        instructions.len(), config.jito_tip);

    // Strategy: Send via BOTH RPC and Jito simultaneously for max coverage
    let spam_count = 8;

    // Use cached blockhash from background updater (0ms, no RPC call).
    // The updater runs every 400ms using default commitment to match what RPC expects.
    let mut blockhash = get_blockhash();
    if blockhash == solana_sdk::hash::Hash::default() {
        // Fallback: if cache is empty (e.g. in test tools), fetch directly
        blockhash = rpc_client.get_latest_blockhash().await.unwrap_or_default();
    }

    let message = match Message::try_compile(&user, &instructions, &[], blockhash) {
        Ok(m) => m,
        Err(e) => {
            log_info!("[WARN] compile err: {}", e);
            return;
        }
    };
    let transaction = match VersionedTransaction::try_new(VersionedMessage::V0(message), &[&config.keypair]) {
        Ok(t) => t,
        Err(e) => {
            log_info!("[WARN] sign err: {}", e);
            return;
        }
    };

    let serialized = bincode::serialize(&transaction).unwrap();
    let encoded = bs58::encode(&serialized).into_string();

    // Log the transaction size to monitor the 1232-byte limit
    log_info!("[SNIPE] TX Size: {} bytes (Limit: 1232 bytes)", serialized.len());

    for attempt in 1..=spam_count {
        // Send via RPC
        let rpc_clone = rpc_client.clone();
        let attempt_num = attempt;
        let tx_clone = transaction.clone();
        // Fill-first mode: always skip preflight for maximum speed.
        let skip_pf = true;
        tokio::spawn(async move {
            match rpc_clone.send_transaction_with_config(
                &tx_clone,
                RpcSendTransactionConfig {
                    skip_preflight: skip_pf,
                    max_retries: Some(0),
                    ..Default::default()
                },
            ).await {
                Ok(sig) => {
                    log_info!("[OK] RPC #{} sent: {}", attempt_num, sig);
                    log_info!("   https://solscan.io/tx/{}", sig);
                }
                Err(e) => {
                    log_info!("[WARN] RPC #{} error: {}", attempt_num, e);
                }
            }
        });

        // Send via Jito (all 4 regional endpoints simultaneously)
        let encoded_clone = encoded.clone();
        let attempt_num2 = attempt;
        tokio::spawn(async move {
            match crate::transaction::send_via_jito(&[encoded_clone]).await {
                Ok(bundle_id) => {
                    log_info!("[OK] Jito #{} bundle: {}", attempt_num2, bundle_id);
                }
                Err(e) => {
                    log_info!("[WARN] Jito #{}: {}", attempt_num2, e);
                }
            }
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(35)).await;
    }

    log_info!("[SNIPE] Done - fired {} attempts via RPC + Jito", spam_count);
}
