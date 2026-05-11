// ==================== TEST: Swap with a known pool ====================
//
// Usage:
//   cargo run --bin test_swap -- <pool_creation_tx_signature>
//
// Example:
//   cargo run --bin test_swap -- 4NJzePDuESZY5HtuD4nJP...
//
// What it does:
//   1. Parse pool accounts from the given TX signature
//   2. Call handle_buy() with that pool
//   3. Verify the swap TX is correct (DRY_RUN=true) or send it live

use slidefun_raydium_snipe::{blockhash, config, handler, pool};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{hash::Hash, signer::Signer};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

macro_rules! log {
    ($($arg:tt)*) => {
        println!("[TEST] {}", format!($($arg)*));
    };
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();
    let signature = if args.len() >= 2 {
        args[1].clone()
    } else {
        eprintln!("Usage: cargo run --bin test_swap -- <pool_creation_tx_signature>");
        eprintln!("Example:");
        eprintln!("  cargo run --bin test_swap -- 4NJzePDuESZY5HtuD4nJP...");
        std::process::exit(1);
    };

    let config = Arc::new(config::Config::from_env());

    println!("===========================================");
    println!("  TEST SWAP — AMM V4 Pool");
    println!("===========================================");
    log!("Wallet:     {}", config.keypair.pubkey());
    log!("SOL amount: {} SOL", config.sol_amount);
    log!("Jito tip:   {} SOL", config.jito_tip);
    log!("DRY_RUN:    {}", config.dry_run);
    log!("Pool TX:    {}", signature);
    println!("");

    let rpc_url = format!(
        "https://mainnet.helius-rpc.com/?api-key={}",
        config.helius_api_key
    );
    let rpc_client = Arc::new(RpcClient::new(rpc_url.clone()));

    // Start blockhash updater
    {
        let url = rpc_url.clone();
        tokio::spawn(async move {
            blockhash::blockhash_updater(url).await;
        });
    }

    // Wait for blockhash
    log!("[1/3] Getting blockhash...");
    while blockhash::get_blockhash() == Hash::default() {
        sleep(Duration::from_millis(10)).await;
    }
    log!("[1/3] Blockhash ready ✅");

    // Parse pool
    log!("[2/3] Parsing pool from TX...");
    let pool_info = match pool::get_pool_info(&rpc_client, &signature, config.raydium_program()).await {
        Some(info) => {
            log!("[2/3] Pool parsed ✅");
            log!("   AMM ID:      {}", info.amm_id);
            log!("   Token mint:  {}", info.base_mint);
            log!("   Base vault:  {}", info.base_vault);
            log!("   Quote vault: {}", info.quote_vault);
            log!("   Market ID:   {}", info.market_id);
            log!("   Market bids: {}", info.market_bids);
            log!("   Market asks: {}", info.market_asks);
            log!(
                "   Pool SOL:    {:.4} SOL",
                info.pool_sol_amount as f64 / 1e9
            );
            info
        }
        None => {
            eprintln!("[ERROR] Failed to parse pool.");
            eprintln!("  - Is this an AMM V4 initialize2 TX?");
            std::process::exit(1);
        }
    };

    // Fire swap
    log!("[3/3] Firing swap (ata_pre_created=false)...");
    handler::handle_buy(&config, rpc_client, pool_info, false).await;

    println!("");
    println!("===========================================");
    println!("  TEST COMPLETE");
    println!("===========================================");

    Ok(())
}
