use bincode;
use bs58;
/// Debug TX encoding: print the raw bytes to verify format is correct.
use dotenvy;
use slidefun_raydium_snipe::config::Config;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    message::{v0::Message, VersionedMessage},
    pubkey::Pubkey,
    signer::{keypair::Keypair, Signer},
    system_instruction,
    transaction::VersionedTransaction,
};
use std::str::FromStr;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let config = Config::from_env();
    let rpc_url = format!(
        "https://mainnet.helius-rpc.com/?api-key={}",
        config.helius_api_key
    );
    let rpc = Arc::new(RpcClient::new(rpc_url));

    let user = config.keypair.pubkey();
    let jito_tip = Pubkey::from_str("96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5").unwrap();

    let bh = rpc.get_latest_blockhash().await.unwrap();

    // Build simple tip TX
    let ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(3_000),
        system_instruction::transfer(&user, &jito_tip, 1_000_000),
    ];
    let msg = Message::try_compile(&user, &ixs, &[], bh).unwrap();
    let tx = VersionedTransaction::try_new(VersionedMessage::V0(msg), &[&config.keypair]).unwrap();

    // Encode both ways
    let bincode_bytes = bincode::serialize(&tx).unwrap();
    let b58_encoded = bs58::encode(&bincode_bytes).into_string();

    println!("=== Bincode serialized ===");
    println!("Total bytes: {}", bincode_bytes.len());
    println!(
        "First 10 bytes (hex): {:?}",
        bincode_bytes[..10]
            .iter()
            .map(|b| format!("{:#04x}", b))
            .collect::<Vec<_>>()
    );
    println!("Base58 encoded len: {}", b58_encoded.len());
    println!("Base58 first 30 chars: {}", &b58_encoded[..30]);
    println!();

    // Now decode and re-check
    let decoded = bs58::decode(&b58_encoded).into_vec().unwrap();
    println!("Decoded bytes == original: {}", decoded == bincode_bytes);
    println!();

    // Expected: byte[0] = 0x01 (1 signature), bytes[1..65] = signature, byte[65] = 0x80 (V0 message)
    println!(
        "byte[0] (num signatures): {:#04x} (expected: 0x01)",
        bincode_bytes[0]
    );
    println!(
        "byte[65] (message version): {:#04x} (expected: 0x80 for V0)",
        bincode_bytes[65]
    );
    println!();

    // Also try sending this TX directly to verify it works
    println!("Testing direct send of tip TX via RPC...");
    match rpc.send_transaction(&tx).await {
        Ok(sig) => println!("✅ Direct send OK: {}", sig),
        Err(e) => println!("❌ Direct send error: {}", e),
    }
}
