// ==================== SLIDE-FUN CREATION SNIPE ====================
//
// This module listens to the Slide.fun Meme Program for `create_bonding_curve`
// instructions. When a new token is created, we immediately send a `buy`
// instruction to the same program to purchase tokens on the bonding curve.
//
// Flow:
//   1. Listen for `create_bonding_curve` logs from Slide.fun program
//   2. Parse the transaction to extract:
//      - token_mint (the new token)
//      - payment mint (usually WSOL / native SOL)
//   3. Derive PDA accounts (config, bonding_curve, vaults)
//   4. Build + send a `buy` instruction via the Slide.fun program
//
// Slide.fun `buy` instruction (from IDL):
//   Discriminator: [102, 6, 61, 18, 1, 218, 235, 234]
//   Accounts in order:
//     [0]  user                  (writable, signer)
//     [1]  config                (PDA: seeds = b"config")
//     [2]  bonding_curve         (writable, PDA: seeds = [b"bonding_curve", token])
//     [3]  token                 (the token mint)
//     [4]  payment               (WSOL mint)
//     [5]  bonding_curve_token_ata (writable, PDA: ATA of bonding_curve for token)
//     [6]  bonding_curve_payment_ata (writable, PDA: ATA of bonding_curve for payment)
//     [7]  user_token_ata        (writable, PDA: ATA of user for token)
//     [8]  user_payment_ata      (writable, PDA: ATA of user for payment/WSOL)
//     [9]  fee_to_payment_ata    (writable, PDA: ATA of config.fee_to for payment)
//     [10] associated_token_program
//     [11] system_program
//     [12] token_program
//   Args: amount (u64), is_exact_in (bool), input_number (u64)

use crate::{constants, log_info};
use crate::config::Config;
use crate::transaction::send_via_jito;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::{RpcSendTransactionConfig, RpcTransactionConfig};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    instruction::{AccountMeta, Instruction},
    message::{v0::Message, VersionedMessage},
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signature::Signature,
    signer::Signer,
    system_instruction,
    transaction::VersionedTransaction,
};
use solana_transaction_status_client_types::UiTransactionEncoding;
use spl_associated_token_account::{
    get_associated_token_address,
    instruction::create_associated_token_account_idempotent,
};
use std::str::FromStr;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

// ==============================================================
// PDA derivation helpers
// ==============================================================

fn get_slidefun_program() -> Pubkey {
    Pubkey::from_str(constants::slidefun_program()).unwrap()
}

/// Derive config PDA: seeds = [b"config"]
pub fn derive_config_pda() -> Pubkey {
    let (pda, _) = Pubkey::find_program_address(
        &[constants::SLIDEFUN_CONFIG_SEED],
        &get_slidefun_program(),
    );
    pda
}

/// Derive bonding_curve PDA: seeds = [b"bonding_curve", token_mint]
pub fn derive_bonding_curve_pda(token_mint: &Pubkey) -> Pubkey {
    let (pda, _) = Pubkey::find_program_address(
        &[constants::SLIDEFUN_BONDING_CURVE_SEED, token_mint.as_ref()],
        &get_slidefun_program(),
    );
    pda
}

// ==============================================================
// Buy instruction builder
// ==============================================================

/// Build the Slide.fun `buy` instruction.
/// - sol_amount: how many SOL to spend (fractional, e.g. 0.05)
/// - min_token_out: minimum tokens to receive (slippage protection, 0 = no min)
pub fn build_slidefun_buy_instruction(
    user: &Pubkey,
    token_mint: &Pubkey,
    payment_mint: &Pubkey,   // usually WSOL / So1111...
    fee_to: &Pubkey,         // from config account on-chain, fetched separately
    sol_amount_lamports: u64,
    min_token_out: u64,
) -> Instruction {
    let slidefun_program = get_slidefun_program();
    let token_program = Pubkey::from_str(constants::TOKEN_PROGRAM).unwrap();
    let assoc_token_program =
        Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL").unwrap();
    let system_program = solana_sdk::system_program::ID;

    let config_pda = derive_config_pda();
    let bonding_curve_pda = derive_bonding_curve_pda(token_mint);

    // ATAs
    let bonding_curve_token_ata = get_associated_token_address(&bonding_curve_pda, token_mint);
    let bonding_curve_payment_ata = get_associated_token_address(&bonding_curve_pda, payment_mint);
    let user_token_ata = get_associated_token_address(user, token_mint);
    let user_payment_ata = get_associated_token_address(user, payment_mint);
    let fee_to_payment_ata = get_associated_token_address(fee_to, payment_mint);

    let accounts = vec![
        AccountMeta::new(*user, true),                   // [0] user
        AccountMeta::new_readonly(config_pda, false),    // [1] config
        AccountMeta::new(bonding_curve_pda, false),      // [2] bonding_curve
        AccountMeta::new_readonly(*token_mint, false),   // [3] token
        AccountMeta::new_readonly(*payment_mint, false), // [4] payment
        AccountMeta::new(bonding_curve_token_ata, false),// [5] bonding_curve_token_ata
        AccountMeta::new(bonding_curve_payment_ata, false), // [6] bonding_curve_payment_ata
        AccountMeta::new(user_token_ata, false),         // [7] user_token_ata
        AccountMeta::new(user_payment_ata, false),       // [8] user_payment_ata
        AccountMeta::new(fee_to_payment_ata, false),     // [9] fee_to_payment_ata
        AccountMeta::new_readonly(assoc_token_program, false), // [10] associated_token_program
        AccountMeta::new_readonly(system_program, false),// [11] system_program
        AccountMeta::new_readonly(token_program, false), // [12] token_program
    ];

    // Instruction data: 8-byte discriminator + amount (u64) + is_exact_in (bool=true) + input_number (u64=0)
    let mut data = constants::SLIDEFUN_BUY_DISCRIMINATOR.to_vec();
    data.extend_from_slice(&sol_amount_lamports.to_le_bytes()); // amount
    data.push(1u8); // is_exact_in = true (we specify exact SOL in, get min tokens out)
    data.extend_from_slice(&min_token_out.to_le_bytes()); // input_number (min tokens out)

    Instruction {
        program_id: slidefun_program,
        accounts,
        data,
    }
}

// ==============================================================
// Config account fetcher (to get fee_to address)
// ==============================================================

/// Fetch the `fee_to` pubkey from the Slide.fun Config PDA account.
/// Config account layout (after 8-byte discriminator):
///   admin: Pubkey (32)
///   fee_to: Pubkey (32)  <-- at offset 8+32 = 40
pub async fn fetch_fee_to(rpc_client: &RpcClient) -> Option<Pubkey> {
    let config_pda = derive_config_pda();
    match rpc_client.get_account(&config_pda).await {
        Ok(account) => {
            let data = &account.data;
            if data.len() < 72 {
                log_info!("[SFSNIPE] Config account too short: {} bytes", data.len());
                return None;
            }
            // bytes 8..=39 = admin, bytes 40..=71 = fee_to
            let fee_to_bytes: [u8; 32] = data[40..72].try_into().ok()?;
            Some(Pubkey::from(fee_to_bytes))
        }
        Err(e) => {
            log_info!("[SFSNIPE] Failed to fetch config PDA: {}", e);
            None
        }
    }
}

// ==============================================================
// Main buy flow
// ==============================================================

/// Called when a new Slide.fun token is detected.
/// Builds and sends a buy transaction against the Slide.fun bonding curve.
pub async fn handle_slidefun_buy(
    config: &Config,
    rpc_client: Arc<RpcClient>,
    token_mint: &str,
) {
    let user = config.keypair.pubkey();
    let sol_lamports = (config.slidefun_pump_amount * LAMPORTS_PER_SOL as f64) as u64;
    let jito_lamports = (config.jito_tip * LAMPORTS_PER_SOL as f64) as u64;

    log_info!("[SFSNIPE] >> Slide.fun BUY token: {}", token_mint);
    log_info!("[SFSNIPE]    SOL amount: {} SOL ({} lamports)", config.slidefun_pump_amount, sol_lamports);

    let token_mint_pk = match Pubkey::from_str(token_mint) {
        Ok(pk) => pk,
        Err(e) => { log_info!("[SFSNIPE] Invalid token mint: {}", e); return; }
    };
    let payment_mint = Pubkey::from_str(constants::WSOL_MINT).unwrap();
    let token_program = Pubkey::from_str(constants::TOKEN_PROGRAM).unwrap();
    let user_payment_ata = get_associated_token_address(&user, &payment_mint);

    // Fetch fee_to from config PDA
    let fee_to = match fetch_fee_to(&rpc_client).await {
        Some(pk) => pk,
        None => {
            log_info!("[SFSNIPE] Could not get fee_to — aborting buy");
            return;
        }
    };

    if config.dry_run {
        log_info!("[SFSNIPE] DRY RUN — not sending transaction");
        log_info!("[SFSNIPE]    Would buy {} SOL of {}", config.slidefun_pump_amount, token_mint);
        log_info!("[SFSNIPE]    fee_to: {}", fee_to);
        return;
    }

    let blockhash = crate::blockhash::get_blockhash();

    // Build instruction list:
    // 1. ComputeBudget
    // 2. Create user WSOL ATA (idempotent)
    // 3. Transfer SOL -> WSOL ATA
    // 4. SyncNative (to make it valid WSOL)
    // 5. Create user token ATA (idempotent)
    // 6. The buy instruction
    // 7. Jito tip

    let jito_tip_address = Pubkey::from_str(crate::constants::jito_tip_address()).unwrap();

    let buy_ix = build_slidefun_buy_instruction(
        &user,
        &token_mint_pk,
        &payment_mint,
        &fee_to,
        sol_lamports,
        0, // min_token_out = 0 (no slippage protection, max speed)
    );

    let ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(config.cu_limit),
        ComputeBudgetInstruction::set_compute_unit_price(config.priority_fee),
        // Prepare WSOL
        create_associated_token_account_idempotent(&user, &user, &payment_mint, &token_program),
        system_instruction::transfer(&user, &user_payment_ata, sol_lamports),
        Instruction {
            program_id: token_program,
            accounts: vec![AccountMeta::new(user_payment_ata, false)],
            data: vec![17], // SyncNative
        },
        // Create user token ATA
        create_associated_token_account_idempotent(&user, &user, &token_mint_pk, &token_program),
        // BUY!
        buy_ix,
        // Jito tip
        system_instruction::transfer(&user, &jito_tip_address, jito_lamports),
    ];

    let msg = match Message::try_compile(&user, &ixs, &[], blockhash) {
        Ok(m) => m,
        Err(e) => { log_info!("[SFSNIPE] Message compile error: {}", e); return; }
    };

    let tx = match VersionedTransaction::try_new(VersionedMessage::V0(msg), &[&config.keypair]) {
        Ok(t) => t,
        Err(e) => { log_info!("[SFSNIPE] Sign error: {}", e); return; }
    };

    let serialized = bincode::serialize(&tx).unwrap_or_default();
    log_info!("[SFSNIPE] TX size: {} bytes (limit: 1232)", serialized.len());
    if serialized.len() > 1232 {
        log_info!("[SFSNIPE] ERROR: TX too large! Aborting.");
        return;
    }

    let encoded = bs58::encode(&serialized).into_string();

    log_info!("[SFSNIPE] 🚀 Firing Slide.fun buy ({} SOL)...", config.slidefun_pump_amount);

    // Spam via Jito
    let spam_count = 4;
    for attempt in 1..=spam_count {
        let rpc_clone = rpc_client.clone();
        let tx_clone = tx.clone();

        tokio::spawn(async move {
            // Send via RPC
            let config_rpc = RpcSendTransactionConfig {
                skip_preflight: attempt > 1,
                max_retries: Some(0),
                ..Default::default()
            };
            match rpc_clone.send_transaction_with_config(&tx_clone, config_rpc).await {
                Ok(sig) => log_info!("[SFSNIPE] RPC attempt {} OK: {}", attempt, sig),
                Err(e) => {
                    if attempt == 1 {
                        log_info!("[SFSNIPE] RPC attempt {} preflight error: {}", attempt, e);
                    }
                }
            }
        });

        // Send via Jito bundle
        let encoded_clone2 = encoded.clone();
        tokio::spawn(async move {
            if let Err(e) = send_via_jito(&[encoded_clone2]).await {
                log_info!("[SFSNIPE] Jito attempt {} error: {}", attempt, e);
            }
        });

        if attempt < spam_count {
            sleep(Duration::from_millis(10)).await;
        }
    }

    log_info!("[SFSNIPE] ✅ Slide.fun buy sent! Token: {}", token_mint);
}

// ==============================================================
// Transaction parser: detect new token creation
// ==============================================================

/// Check logs for a `create_bonding_curve` event from Slide.fun.
pub fn is_creation_signal(logs: &[String]) -> bool {
    let logs_str = logs.join(" ");
    logs_str.contains(constants::slidefun_program())
        && logs_str.contains("Instruction: CreateBondingCurve")
}

/// Parse the transaction to extract the new token mint from `create_bonding_curve`.
/// Returns (token_mint, creator_pubkey)
pub async fn extract_new_token_and_creator(
    rpc_client: &RpcClient,
    signature: &str,
) -> Option<(String, String)> {
    let slidefun_program = get_slidefun_program();
    let sig = Signature::from_str(signature).ok()?;
    let config = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Base64),
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(0),
    };

    for attempt in 0..5 {
        match rpc_client.get_transaction_with_config(&sig, config.clone()).await {
            Ok(tx) => {
                if let Some(decoded) = tx.transaction.transaction.decode() {
                    let message = &decoded.message;
                    let mut all_keys = message.static_account_keys().to_vec();

                    if let Some(meta) = &tx.transaction.meta {
                        use solana_transaction_status_client_types::option_serializer::OptionSerializer;
                        if let OptionSerializer::Some(loaded) = &meta.loaded_addresses {
                            for addr in &loaded.writable {
                                if let Ok(pk) = Pubkey::from_str(addr) { all_keys.push(pk); }
                            }
                            for addr in &loaded.readonly {
                                if let Ok(pk) = Pubkey::from_str(addr) { all_keys.push(pk); }
                            }
                        }
                    }

                    let instructions = match message {
                        solana_sdk::message::VersionedMessage::Legacy(m) => &m.instructions,
                        solana_sdk::message::VersionedMessage::V0(m) => &m.instructions,
                    };

                    for ix in instructions {
                        let program_id = all_keys[ix.program_id_index as usize];
                        if program_id != slidefun_program { continue; }

                        // Match create_bonding_curve discriminator
                        if ix.data.len() < 8
                            || ix.data[0..8] != constants::SLIDEFUN_CREATE_BONDING_CURVE_DISCRIMINATOR
                        {
                            continue;
                        }

                        // account[2] = token mint
                        // The creator is typically the fee payer, which is all_keys[0], or we can extract the signer from the ix.accounts if it exists.
                        // On slide.fun IDL (standard pumpfun fork), the user who creates the curve is account 0 of the create_bonding_curve instruction or all_keys[0].
                        // Let's use all_keys[0] which is the fee payer and signer of the transaction.
                        if ix.accounts.len() > 2 {
                            let token_mint = all_keys[ix.accounts[2] as usize];
                            let creator = all_keys[0]; // Fee payer is the creator
                            log_info!("[SFSNIPE] ✅ New token detected: {} by creator: {}", token_mint, creator);
                            return Some((token_mint.to_string(), creator.to_string()));
                        }
                    }
                }
                return None;
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

/// Fallback compatibility
pub async fn extract_new_token(
    rpc_client: &RpcClient,
    signature: &str,
) -> Option<String> {
    extract_new_token_and_creator(rpc_client, signature).await.map(|(mint, _)| mint)
}
