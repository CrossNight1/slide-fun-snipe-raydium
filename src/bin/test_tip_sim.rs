use dotenvy;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    message::{v0::Message, VersionedMessage},
    pubkey::Pubkey,
    signer::{keypair::Keypair, Signer},
    system_instruction,
    transaction::VersionedTransaction,
};
use slidefun_raydium_snipe::config::Config;
use std::str::FromStr;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let config = Config::from_env();
    let rpc_url = format!("https://mainnet.helius-rpc.com/?api-key={}", config.helius_api_key);
    let rpc = Arc::new(RpcClient::new(rpc_url));
    
    let user = config.keypair.pubkey();
    let jito_tip_address = Pubkey::from_str("96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5").unwrap();
    let tip_lamports = (config.jito_tip * 1_000_000_000f64) as u64;
    
    println!("Main wallet : {}", user);
    println!("Jito tip    : {} lamports", tip_lamports);
    
    let bh = rpc.get_latest_blockhash().await.unwrap();
    
    let ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(3_000),
        system_instruction::transfer(&user, &jito_tip_address, tip_lamports),
    ];
    
    let msg = Message::try_compile(&user, &ixs, &[], bh).unwrap();
    let tx = VersionedTransaction::try_new(VersionedMessage::V0(msg), &[&config.keypair]).unwrap();
    
    let sim_cfg = RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: true,
        commitment: Some(CommitmentConfig::confirmed()),
        ..Default::default()
    };
    
    println!("\nSimulating tip TX...");
    match rpc.simulate_transaction_with_config(&tx, sim_cfg).await {
        Err(e) => println!("❌ RPC error: {}", e),
        Ok(r) => {
            if let Some(err) = &r.value.err {
                println!("❌ FAILED: {:?}", err);
                if let Some(logs) = &r.value.logs {
                    for l in logs { println!("  {}", l); }
                }
            } else {
                println!("✅ PASSED (units: {})", r.value.units_consumed.unwrap_or(0));
                println!("Balance before: {} lamports", r.value.accounts.as_ref().map(|_| "N/A").unwrap_or("N/A"));
            }
        }
    }
}
