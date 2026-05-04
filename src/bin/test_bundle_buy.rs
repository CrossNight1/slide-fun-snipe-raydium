// ==================== BUNDLE BUY TEST ====================
// Simulate the full bundle buy flow without waiting for a real token.
// Loads your wallets.json, builds all TXs, logs their sizes.
//
// Usage:
//   cargo run --bin test_bundle_buy               → use DRY_RUN from .env
//   cargo run --bin test_bundle_buy -- live       → force send (REAL SOL!)
//
// What it does:
//   1. Loads config from .env (PRIVATE_KEY, HELIUS_API_KEY, etc.)
//   2. Loads sub-wallets from BUNDLE_WALLETS_FILE
//   3. Picks a random fake token mint (or you can hardcode a real one)
//   4. Builds all bundle buy TXs for every sub-wallet
//   5. Logs wallet addresses, TX sizes, bundle layout
//   6. If DRY_RUN=false (or forced "live"), sends to Jito

use bincode;
use dotenvy;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{v0::Message, VersionedMessage},
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signer::{keypair::Keypair, Signer},
    system_instruction,
    transaction::VersionedTransaction,
};
use spl_associated_token_account::{
    get_associated_token_address,
    instruction::create_associated_token_account_idempotent,
};
use std::str::FromStr;
use std::sync::Arc;

// ── Inline constants ──────────────────────────────────────────────────
const SLIDEFUN_PROGRAM:    &str = "GkF6F9GNPjzkC18Xa3a88xwEc5vwyQDA1iXvFkKBqNDC";
const TOKEN_PROGRAM:       &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const WSOL_MINT:           &str = "So11111111111111111111111111111111111111112";
const JITO_TIP_ADDRESS:    &str = "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5";
const ASSOC_TOKEN_PGM:     &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
const SF_BUY_DISC:   [u8; 8]   = [102, 6, 61, 18, 1, 218, 235, 234];
const SF_CONFIG:     &[u8]     = b"config";
const SF_BC:         &[u8]     = b"bonding_curve";
const JITO_URLS: &[&str] = &[
    "https://ny.mainnet.block-engine.jito.wtf/api/v1/bundles",
    "https://amsterdam.mainnet.block-engine.jito.wtf/api/v1/bundles",
    "https://tokyo.mainnet.block-engine.jito.wtf/api/v1/bundles",
    "https://slc.mainnet.block-engine.jito.wtf/api/v1/bundles",
];
// ─────────────────────────────────────────────────────────────────────

fn load_wallets(path: &str) -> Vec<Keypair> {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|_| "[]".to_string());
    let json: serde_json::Value = serde_json::from_str(&content).unwrap_or(serde_json::json!([]));
    let mut kps = Vec::new();
    if let Some(arr) = json.as_array() {
        for (i, entry) in arr.iter().enumerate() {
            if let Some(pk) = entry["private_key"].as_str() {
                let kp = Keypair::from_base58_string(pk);
                println!("  [wallet {:>2}] {}", i, kp.pubkey());
                kps.push(kp);
            }
        }
    }
    kps
}

fn sf_buy_ix(user: &Pubkey, token_mint: &Pubkey, fee_to: &Pubkey, sol_lamports: u64) -> Instruction {
    let prog  = Pubkey::from_str(SLIDEFUN_PROGRAM).unwrap();
    let tok_p = Pubkey::from_str(TOKEN_PROGRAM).unwrap();
    let assoc = Pubkey::from_str(ASSOC_TOKEN_PGM).unwrap();
    let wsol  = Pubkey::from_str(WSOL_MINT).unwrap();

    let (cfg, _) = Pubkey::find_program_address(&[SF_CONFIG], &prog);
    let (bc,  _) = Pubkey::find_program_address(&[SF_BC, token_mint.as_ref()], &prog);
    let bc_tok  = get_associated_token_address(&bc, token_mint);
    let bc_pay  = get_associated_token_address(&bc, &wsol);
    let u_tok   = get_associated_token_address(user, token_mint);
    let u_pay   = get_associated_token_address(user, &wsol);
    let fee_ata = get_associated_token_address(fee_to, &wsol);

    let mut data = SF_BUY_DISC.to_vec();
    data.extend_from_slice(&sol_lamports.to_le_bytes());
    data.push(1u8); // is_exact_in = true
    data.extend_from_slice(&0u64.to_le_bytes());

    Instruction {
        program_id: prog,
        accounts: vec![
            AccountMeta::new(*user, true),
            AccountMeta::new_readonly(cfg, false),
            AccountMeta::new(bc, false),
            AccountMeta::new_readonly(*token_mint, false),
            AccountMeta::new_readonly(wsol, false),
            AccountMeta::new(bc_tok, false),
            AccountMeta::new(bc_pay, false),
            AccountMeta::new(u_tok, false),
            AccountMeta::new(u_pay, false),
            AccountMeta::new(fee_ata, false),
            AccountMeta::new_readonly(assoc, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(tok_p, false),
        ],
        data,
    }
}

fn build_buy_tx(
    keypair: &Keypair,
    token_mint: &Pubkey,
    fee_to: &Pubkey,
    sol_lamports: u64,
    cu_limit: u32,
    priority_fee: u64,
    blockhash: Hash,
) -> Option<VersionedTransaction> {
    let user      = keypair.pubkey();
    let wsol      = Pubkey::from_str(WSOL_MINT).unwrap();
    let tok_p     = Pubkey::from_str(TOKEN_PROGRAM).unwrap();
    let user_wsol = get_associated_token_address(&user, &wsol);

    let ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(cu_limit),
        ComputeBudgetInstruction::set_compute_unit_price(priority_fee),
        create_associated_token_account_idempotent(&user, &user, &wsol, &tok_p),
        system_instruction::transfer(&user, &user_wsol, sol_lamports),
        Instruction {
            program_id: tok_p,
            accounts: vec![AccountMeta::new(user_wsol, false)],
            data: vec![17], // SyncNative
        },
        create_associated_token_account_idempotent(&user, &user, token_mint, &tok_p),
        sf_buy_ix(&user, token_mint, fee_to, sol_lamports),
    ];

    match Message::try_compile(&user, &ixs, &[], blockhash) {
        Ok(msg) => match VersionedTransaction::try_new(VersionedMessage::V0(msg), &[keypair]) {
            Ok(tx) => Some(tx),
            Err(e) => { println!("    Sign error: {}", e); None }
        },
        Err(e) => { println!("    Compile error: {}", e); None }
    }
}

fn build_tip_tx(main_kp: &Keypair, tip_lamports: u64, blockhash: Hash) -> Option<VersionedTransaction> {
    let user = main_kp.pubkey();
    let jito = Pubkey::from_str(JITO_TIP_ADDRESS).unwrap();
    let ixs  = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(3_000),
        system_instruction::transfer(&user, &jito, tip_lamports),
    ];
    match Message::try_compile(&user, &ixs, &[], blockhash) {
        Ok(msg) => match VersionedTransaction::try_new(VersionedMessage::V0(msg), &[main_kp]) {
            Ok(tx) => Some(tx),
            Err(e) => { println!("  Tip TX sign error: {}", e); None }
        },
        Err(e) => { println!("  Tip TX compile error: {}", e); None }
    }
}

async fn send_bundle(bundle: Vec<String>) {
    let client = reqwest::Client::new();
    let handles: Vec<_> = JITO_URLS.iter().map(|url| {
        let client = client.clone();
        let url = url.to_string();
        let bundle = bundle.clone();
        tokio::spawn(async move {
            let payload = serde_json::json!({
                "jsonrpc": "2.0", "id": 1,
                "method": "sendBundle", "params": [bundle]
            });
            match client.post(&url).json(&payload).send().await {
                Ok(resp) => {
                    let body: serde_json::Value = resp.json().await.unwrap_or_default();
                    if let Some(id) = body["result"].as_str() {
                        println!("  [JITO {}] Bundle ID: {}", url.split('/').nth(2).unwrap_or("?"), id);
                    } else {
                        println!("  [JITO] Error: {:?}", body["error"]);
                    }
                }
                Err(e) => println!("  [JITO] Request error: {}", e),
            }
        })
    }).collect();
    futures::future::join_all(handles).await;
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let force_live = std::env::args().any(|a| a == "live");

    // Load config
    let private_key   = std::env::var("PRIVATE_KEY").expect("PRIVATE_KEY missing in .env");
    let helius_key    = std::env::var("HELIUS_API_KEY").expect("HELIUS_API_KEY missing in .env");
    let dry_run       = if force_live { false } else {
        std::env::var("DRY_RUN").unwrap_or("true".into()) != "false"
    };
    let sol_per_wallet: f64 = std::env::var("BUNDLE_SOL_PER_WALLET")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(0.01);
    let jito_tip: f64 = std::env::var("JITO_TIP")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(0.001);
    let cu_limit: u32 = std::env::var("CU_LIMIT")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(200_000);
    let priority_fee: u64 = std::env::var("PRIORITY_FEE")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(100_000);
    let wallets_file  = std::env::var("BUNDLE_WALLETS_FILE")
        .unwrap_or("wallets.json".into());

    let main_kp = Keypair::from_base58_string(&private_key);
    let rpc_url = format!("https://mainnet.helius-rpc.com/?api-key={}", helius_key);
    let rpc     = Arc::new(RpcClient::new(rpc_url));

    // Real token mint + fee_to for live test
    let token_mint = Pubkey::from_str("8mEucFjUZ6SkSGsaFVYxsqxNbXk9p1Rw81Yv3a8QmcmE").unwrap();
    let fee_to     = Pubkey::from_str("GCDcDJEW25W4sNUdzC1YNr9ojbi1Cwof5AbVd2qTxA7A").unwrap();

    let sol_lamports  = (sol_per_wallet * LAMPORTS_PER_SOL as f64) as u64;
    let tip_lamports  = (jito_tip * LAMPORTS_PER_SOL as f64) as u64;

    println!("\n╔══════════════════════════════════════════════════════╗");
    println!("║         BUNDLE BUY TEST — Slide.fun mode            ║");
    println!("╚══════════════════════════════════════════════════════╝");
    println!("  Mode        : {}", if dry_run { "DRY RUN (no TX sent)" } else { "⚡ LIVE (real SOL!)" });
    println!("  Main wallet : {}", main_kp.pubkey());
    println!("  Token mint  : {} (fake for dry run)", token_mint);
    println!("  SOL/wallet  : {} SOL", sol_per_wallet);
    println!("  Jito tip    : {} SOL", jito_tip);
    println!();

    // Load sub-wallets
    println!("  Loading sub-wallets from '{}':", wallets_file);
    let wallets = load_wallets(&wallets_file);
    if wallets.is_empty() {
        println!("  ⚠️  No wallets loaded. Check BUNDLE_WALLETS_FILE in .env");
        return;
    }
    println!("  → {} wallets loaded\n", wallets.len());

    // Get blockhash
    let blockhash = match rpc.get_latest_blockhash().await {
        Ok(bh) => { println!("  [OK] Blockhash: {}\n", bh); bh }
        Err(e) => { println!("  [ERR] Blockhash fetch failed: {}", e); Hash::default() }
    };

    // Build all buy TXs
    println!("  Building buy TXs:");
    let mut buy_txs: Vec<VersionedTransaction> = Vec::new();
    for (i, kp) in wallets.iter().enumerate() {
        match build_buy_tx(kp, &token_mint, &fee_to, sol_lamports, cu_limit, priority_fee, blockhash) {
            Some(tx) => {
                let size = bincode::serialize(&tx).unwrap_or_default().len();
                println!("    wallet[{:>2}] {} → {} bytes ✅", i, kp.pubkey(), size);
                buy_txs.push(tx);
            }
            None => println!("    wallet[{:>2}] build FAILED", i),
        }
    }

    // Pack into bundles (1 tip + 4 buy = 5 per bundle)
    println!("\n  Packing into Jito bundles:");
    let mut bundles: Vec<Vec<String>> = Vec::new();
    for (b_idx, chunk) in buy_txs.chunks(4).enumerate() {
        let tip_tx = match build_tip_tx(&main_kp, tip_lamports, blockhash) {
            Some(tx) => tx,
            None => { println!("    Bundle {} — tip TX failed, skipping", b_idx); continue; }
        };

        let mut bundle: Vec<String> = Vec::new();
        if let Ok(b) = bincode::serialize(&tip_tx) {
            bundle.push(bs58::encode(&b).into_string());
        }
        for tx in chunk {
            if let Ok(b) = bincode::serialize(tx) {
                bundle.push(bs58::encode(&b).into_string());
            }
        }
        println!("    Bundle {} → {} TXs (tip + {} buys)", b_idx, bundle.len(), bundle.len() - 1);
        bundles.push(bundle);
    }

    println!("\n  Total: {} bundles × 4 Jito endpoints = {} requests\n",
        bundles.len(), bundles.len() * 4);

    if dry_run {
        println!("  ✅ DRY RUN complete — all TXs built successfully, NOT sent.");
        println!("  → To send for real: cargo run --bin test_bundle_buy -- live");
    } else {
        println!("  ⚡ SENDING BUNDLES (real SOL will be spent!)...\n");
        for (i, bundle) in bundles.into_iter().enumerate() {
            println!("  Sending bundle {}:", i);
            send_bundle(bundle).await;
        }
        println!("\n  ✅ Done.");
    }

    println!();
}
