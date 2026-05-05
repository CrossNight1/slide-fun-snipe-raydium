/// Test a single 2-TX bundle: [tip_tx, one_buy_tx].
/// This isolates whether the issue is with multiple TXs or with buy TXs in bundles.
use dotenvy;
use slidefun_raydium_snipe::{bundle_buy, config::Config, pool::get_pool_info};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    message::{v0::Message, VersionedMessage},
    native_token::LAMPORTS_PER_SOL,
    pubkey::Pubkey,
    signer::{keypair::Keypair, Signer},
    system_instruction,
    transaction::VersionedTransaction,
};
use bincode;
use bs58;
use std::str::FromStr;
use std::sync::Arc;

fn encode(tx: &VersionedTransaction) -> String {
    bs58::encode(bincode::serialize(tx).unwrap()).into_string()
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 { eprintln!("Usage: test_single_bundle <TX_SIG>"); std::process::exit(1); }

    let config = Config::from_env();
    let rpc_url = format!("https://mainnet.helius-rpc.com/?api-key={}", config.helius_api_key);
    let rpc = Arc::new(RpcClient::new(rpc_url));

    let pool_info = get_pool_info(&rpc, &args[1]).await.expect("Failed to get pool info");
    let wallets: Vec<Keypair> = config.enabled_bundle_keypairs().into_iter().map(|(k, _)| k).collect();
    let wallet = &wallets[1]; // Use wallet[1] (fresh, hasn't been used yet)

    println!("Wallet: {}", wallet.pubkey());
    let bh = rpc.get_latest_blockhash().await.unwrap();
    println!("Blockhash: {}", bh);

    // Build tip TX
    let user = config.keypair.pubkey();
    let jito_tip = Pubkey::from_str("96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5").unwrap();
    let tip_lamports = (config.jito_tip * LAMPORTS_PER_SOL as f64) as u64;
    let tip_ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(3_000),
        system_instruction::transfer(&user, &jito_tip, tip_lamports),
    ];
    let tip_msg = Message::try_compile(&user, &tip_ixs, &[], bh).unwrap();
    let tip_tx = VersionedTransaction::try_new(VersionedMessage::V0(tip_msg), &[&config.keypair]).unwrap();

    // Build buy TX for wallet[1]
    let sol_lamports = (config.bundle_sol_per_wallet * LAMPORTS_PER_SOL as f64) as u64;
    let buy_tx = bundle_buy::build_raydium_buy_tx_for_wallet(
        wallet, &pool_info, sol_lamports, 0, config.cu_limit, config.priority_fee, bh,
    ).expect("Failed to build buy TX");

    println!("Tip TX encoded len: {} chars", encode(&tip_tx).len());
    println!("Buy TX encoded len: {} chars", encode(&buy_tx).len());

    let bundle = vec![encode(&tip_tx), encode(&buy_tx)];

    // Send to London (currently working)
    let client = reqwest::Client::new();
    let resp = client.post("https://london.mainnet.block-engine.jito.wtf/api/v1/bundles")
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"sendBundle","params":[bundle]}))
        .send().await.unwrap()
        .json::<serde_json::Value>().await.unwrap();

    if let Some(id) = resp["result"].as_str() {
        println!("✅ Bundle sent: {}", id);

        // Poll status
        for secs in [2u64, 5, 8, 12] {
            tokio::time::sleep(tokio::time::Duration::from_secs(secs)).await;
            let st = client.post("https://singapore.mainnet.block-engine.jito.wtf/api/v1/bundles")
                .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"getInflightBundleStatuses","params":[[id]]}))
                .send().await.unwrap()
                .json::<serde_json::Value>().await.unwrap();
            let vals = st["result"]["value"].as_array();
            if let Some(arr) = vals {
                if let Some(v) = arr.first() {
                    println!("[+{}s] status={} slot={:?}", secs, v["status"], v["landed_slot"]);
                    if v["status"] == "landed" || v["status"] == "failed" { break; }
                }
            } else {
                println!("[+{}s] null (bundle processed/expired)", secs);
            }
        }

        // Final: check wallet[1] balance
        let helius = format!("https://mainnet.helius-rpc.com/?api-key={}", config.helius_api_key);
        let rpc2 = Arc::new(RpcClient::new(helius));
        let bal = rpc2.get_balance(&wallet.pubkey()).await.unwrap_or(0);
        println!("\nWallet[1] final balance: {} lamports ({:.6} SOL)", bal, bal as f64 / 1e9);
        println!("Started at: 10000000 lamports (0.010000 SOL)");
        if bal < 10_000_000 { println!("✅ Buy TX LANDED (balance decreased)"); }
        else { println!("❌ Buy TX did NOT land (balance unchanged)"); }
    } else {
        println!("❌ Bundle error: {:?}", resp);
    }
}
