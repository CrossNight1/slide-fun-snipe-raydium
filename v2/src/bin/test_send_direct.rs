/// Send a single buy TX directly via RPC (not Jito bundle) to verify TX construction is correct.
use dotenvy;
use slidefun_raydium_snipe::{bundle_buy, config::Config, pool::get_pool_info};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::native_token::LAMPORTS_PER_SOL;
use solana_sdk::signer::{keypair::Keypair, Signer};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo run --bin test_send_direct -- <POOL_INIT_TX_SIG>");
        std::process::exit(1);
    }

    let config = Config::from_env();
    let rpc_url = format!(
        "https://mainnet.helius-rpc.com/?api-key={}",
        config.helius_api_key
    );
    let rpc = Arc::new(RpcClient::new(rpc_url));

    println!("Fetching pool info...");
    let pool_info = match get_pool_info(&rpc, &args[1], config.raydium_program()).await {
        Some(p) => {
            println!("AMM: {} | Token: {}", p.amm_id, p.base_mint);
            p
        }
        None => {
            eprintln!("Could not parse pool info");
            std::process::exit(1);
        }
    };

    let wallets: Vec<Keypair> = config
        .enabled_bundle_keypairs()
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    if wallets.is_empty() {
        eprintln!("No wallets");
        std::process::exit(1);
    }

    let bh = rpc.get_latest_blockhash().await.unwrap();
    let sol_lamports = (config.app.slidefun_pump_amount * LAMPORTS_PER_SOL as f64) as u64;

    let tx = match bundle_buy::build_raydium_buy_tx_for_wallet(
        &wallets[0],
        &pool_info,
        sol_lamports,
        0,
        config.cu_limit,
        config.priority_fee,
        bh,
    ) {
        Some(t) => t,
        None => {
            eprintln!("Could not build TX");
            std::process::exit(1);
        }
    };

    println!("Sending TX for wallet[0]: {}", wallets[0].pubkey());
    println!("Amount: {} SOL", config.app.slidefun_pump_amount);

    match rpc.send_transaction(&tx).await {
        Ok(sig) => {
            println!("✅ TX sent: {}", sig);
            println!("   Solscan: https://solscan.io/tx/{}", sig);

            // Wait and confirm
            println!("   Waiting for confirmation...");
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            match rpc
                .get_signature_status_with_commitment(&sig, CommitmentConfig::confirmed())
                .await
            {
                Ok(Some(Ok(()))) => println!("   ✅ CONFIRMED!"),
                Ok(Some(Err(e))) => println!("   ❌ Failed on-chain: {:?}", e),
                Ok(None) => println!("   ⏳ Not confirmed yet (check Solscan)"),
                Err(e) => println!("   ❌ Error checking status: {}", e),
            }
        }
        Err(e) => println!("❌ Send error: {}", e),
    }
}
