// ==================== WALLET UTILITIES ====================
//
// Pre-funding and balance management for the main wallet's WSOL account.
// Keeping this here prevents main.rs from becoming a dumping ground.

use std::str::FromStr;

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{v0::Message, VersionedMessage},
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signer::Signer,
    system_instruction,
    transaction::VersionedTransaction,
};
use spl_associated_token_account::{
    get_associated_token_address,
    instruction::create_associated_token_account_idempotent,
};

use crate::{config::Config, constants, log_info};

/// Pre-fund the main wallet's WSOL account with `config.sol_amount` SOL.
///
/// This is called once at startup (live mode only). WSOL is used as input for
/// Raydium swaps. The ATA is created idempotently so repeated restarts are safe.
pub async fn prefund_wsol(config: &Config, rpc_client: &RpcClient) {
    if config.dry_run {
        log_info!("[WSOL] DRY_RUN=true → skipping WSOL pre-fund");
        return;
    }

    let user = config.keypair.pubkey();
    let wsol_mint = Pubkey::from_str(constants::WSOL_MINT).unwrap();
    let token_program = Pubkey::from_str(constants::TOKEN_PROGRAM).unwrap();
    let user_wsol_account = get_associated_token_address(&user, &wsol_mint);
    let sol_lamports = (config.sol_amount * LAMPORTS_PER_SOL as f64) as u64;

    log_info!("[WSOL] Pre-funding {:.4} SOL into WSOL ATA...", config.sol_amount);

    let blockhash: Hash = match rpc_client.get_latest_blockhash().await {
        Ok(bh) => bh,
        Err(e) => {
            log_info!("[WSOL] Could not fetch blockhash: {}", e);
            return;
        }
    };

    let ixs = vec![
        #[allow(deprecated)]
        solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_limit(100_000),
        #[allow(deprecated)]
        solana_sdk::compute_budget::ComputeBudgetInstruction::set_compute_unit_price(10_000),
        create_associated_token_account_idempotent(&user, &user, &wsol_mint, &token_program),
        system_instruction::transfer(&user, &user_wsol_account, sol_lamports),
        Instruction {
            program_id: token_program,
            accounts: vec![AccountMeta::new(user_wsol_account, false)],
            data: vec![17], // SyncNative
        },
    ];

    let msg = match Message::try_compile(&user, &ixs, &[], blockhash) {
        Ok(m) => m,
        Err(e) => {
            log_info!("[WSOL] Message compile error: {}", e);
            return;
        }
    };

    let tx = match VersionedTransaction::try_new(VersionedMessage::V0(msg), &[&config.keypair]) {
        Ok(t) => t,
        Err(e) => {
            log_info!("[WSOL] Sign error: {}", e);
            return;
        }
    };

    match rpc_client.send_and_confirm_transaction(&tx).await {
        Ok(sig) => log_info!("[WSOL] ✅ Pre-funded OK — TX: {}", sig),
        Err(e) => log_info!("[WSOL] Pre-fund failed (ATA may already exist): {}", e),
    }
}
