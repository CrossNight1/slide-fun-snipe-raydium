// ==================== GRADUATION DETECTION ====================
// Detect Slide.fun token graduations by monitoring the program's logs.
//
// The graduation flow from slide-fun-be source:
//   BondingCurveEnded → withdrawToken (calls migrate()) → addDex (createMarketAndPoolV4)
//
// We detect at step 2 (migrate), BEFORE step 3 (pool creation).
// This gives us time to:
//   1. Know the token mint in advance
//   2. Pre-create the ATA for the token (removes 1 instruction from swap TX)

use crate::{log_info, pool::mint_token_program_for_mint};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcSendTransactionConfig, RpcTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    message::{v0::Message, VersionedMessage},
    pubkey::Pubkey,
    signature::Signature,
    signer::{keypair::Keypair, Signer},
    transaction::VersionedTransaction,
};
use solana_transaction_status_client_types::UiTransactionEncoding;
use spl_associated_token_account::{
    get_associated_token_address_with_program_id,
    instruction::create_associated_token_account_idempotent,
};
use std::str::FromStr;
use tokio::time::{sleep, Duration};

/// Pre-create the ATA for a graduating token.
/// Called as soon as we detect `migrate` — before the Raydium pool exists.
/// This removes 1 instruction (~20ms) from the critical swap TX.
pub async fn pre_create_ata(rpc_client: &RpcClient, keypair: &Keypair, token_mint: &str) {
    let token_mint_pk = match Pubkey::from_str(token_mint) {
        Ok(pk) => pk,
        Err(_) => return,
    };
    let user = keypair.pubkey();
    let token_program = mint_token_program_for_mint(rpc_client, &token_mint_pk).await;
    let ata = get_associated_token_address_with_program_id(&user, &token_mint_pk, &token_program);

    // Check if ATA already exists
    if rpc_client.get_account(&ata).await.is_ok() {
        log_info!("[GRAD] ATA already exists for {}", token_mint);
        return;
    }

    let ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(50_000),
        ComputeBudgetInstruction::set_compute_unit_price(200_000), // High priority
        create_associated_token_account_idempotent(&user, &user, &token_mint_pk, &token_program),
    ];

    let blockhash = match rpc_client.get_latest_blockhash().await {
        Ok(bh) => bh,
        Err(e) => {
            log_info!("[GRAD] ATA pre-create: blockhash error: {}", e);
            return;
        }
    };

    let msg = match Message::try_compile(&user, &ixs, &[], blockhash) {
        Ok(m) => m,
        Err(e) => {
            log_info!("[GRAD] ATA pre-create: compile error: {}", e);
            return;
        }
    };
    let tx = match VersionedTransaction::try_new(VersionedMessage::V0(msg), &[keypair]) {
        Ok(t) => t,
        Err(e) => {
            log_info!("[GRAD] ATA pre-create: sign error: {}", e);
            return;
        }
    };

    log_info!("[GRAD] Pre-creating ATA for {}...", token_mint);
    match rpc_client
        .send_transaction_with_config(
            &tx,
            RpcSendTransactionConfig {
                skip_preflight: true,
                max_retries: Some(3),
                ..Default::default()
            },
        )
        .await
    {
        Ok(sig) => log_info!("[GRAD] ✅ ATA pre-created: {}", sig),
        Err(e) => log_info!("[GRAD] ATA pre-create failed (may exist): {}", e),
    }
}

/// Extract token mint addresses from a Slide.fun transaction.
/// We look for the `migrate` instruction account layout and extract slot [3] = token mint.
pub async fn extract_graduating_token(
    rpc_client: &RpcClient,
    signature: &str,
    logs: &[String],
    program_id: &Pubkey,
) -> Option<String> {
    let slidefun_program = *program_id;

    // Verify it's actually a migrate instruction
    // Anchor programs emit: "Program log: Instruction: Migrate"
    let logs_joined = logs.join(" ");
    let is_migrate = logs_joined.contains(&program_id.to_string())
        && logs_joined.contains("Instruction: Migrate");

    if !is_migrate {
        return None;
    }

    log_info!("[GRAD] Detected migrate instruction! Parsing token...");

    let sig = Signature::from_str(signature).ok()?;
    let config = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Base64),
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(0),
    };

    for attempt in 0..5 {
        match rpc_client
            .get_transaction_with_config(&sig, config.clone())
            .await
        {
            Ok(tx) => {
                if let Some(decoded) = tx.transaction.transaction.decode() {
                    let message = &decoded.message;
                    let mut all_account_keys = message.static_account_keys().to_vec();

                    if let Some(meta) = &tx.transaction.meta {
                        use solana_transaction_status_client_types::option_serializer::OptionSerializer;
                        if let OptionSerializer::Some(loaded) = &meta.loaded_addresses {
                            for addr in &loaded.writable {
                                if let Ok(pubkey) = Pubkey::from_str(addr) {
                                    all_account_keys.push(pubkey);
                                }
                            }
                            for addr in &loaded.readonly {
                                if let Ok(pubkey) = Pubkey::from_str(addr) {
                                    all_account_keys.push(pubkey);
                                }
                            }
                        }
                    }

                    let instructions = match message {
                        solana_sdk::message::VersionedMessage::Legacy(m) => &m.instructions,
                        solana_sdk::message::VersionedMessage::V0(m) => &m.instructions,
                    };

                    for ix in instructions {
                        let program_id = all_account_keys[ix.program_id_index as usize];
                        if program_id != slidefun_program {
                            continue;
                        }

                        // Verify by Anchor discriminator from IDL:
                        // migrate discriminator = [155, 234, 231, 146, 236, 158, 162, 30]
                        // First 8 bytes of instruction data must match.
                        let migr_disc: [u8; 8] = crate::constants::SLIDEFUN_MIGRATE_DISCRIMINATOR;
                        if ix.data.len() < 8 || ix.data[0..8] != migr_disc {
                            continue; // Not the migrate instruction
                        }

                        // Verified: migrate instruction account layout (IDL lines 556-738):
                        // [0] migration_authority (writable, signer)
                        // [1] config PDA
                        // [2] bonding_curve PDA
                        // [3] token              ← TOKEN MINT ✅
                        // [4] payment (WSOL)
                        // [5] bonding_curve_token_ata
                        // [6] bonding_curve_payment_ata
                        // [7] migration_token_ata
                        // [8] migration_payment_ata
                        // [9] associated_token_program
                        // [10] system_program
                        // [11] token_program
                        if ix.accounts.len() >= 5 {
                            let token_mint = all_account_keys[ix.accounts[3] as usize];
                            log_info!("[GRAD] ✅ Token graduating: {}", token_mint);
                            log_info!("[GRAD]    TX: {}", signature);
                            return Some(token_mint.to_string());
                        }
                    }
                }
            }
            Err(_) => {
                if attempt < 4 {
                    sleep(Duration::from_millis(10)).await;
                }
            }
        }
    }

    None
}

/// Fast pre-filter: check logs without RPC call
/// Anchor logs 'Program log: Instruction: Migrate' for migrate instruction.
/// Must be from Slide.fun program AND contain Anchor migrate log
pub fn is_graduation_signal(logs: &[String], program_id_str: &str) -> bool {
    let logs_str = logs.join(" ");
    logs_str.contains(program_id_str) && logs_str.contains("Instruction: Migrate")
}
