/// Test MINIMAL Jito bundle: just 2 simple transactions from main wallet.
/// If even this doesn't land → issue is with bundle encoding/submission.
use dotenvy;
use slidefun_raydium_snipe::{config::Config, transaction::send_via_jito};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    message::{v0::Message, VersionedMessage},
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
    let bytes = bincode::serialize(tx).unwrap();
    bs58::encode(&bytes).into_string()
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let config = Config::from_env();
    let rpc_url = format!("https://mainnet.helius-rpc.com/?api-key={}", config.helius_api_key);
    let rpc = Arc::new(RpcClient::new(rpc_url));

    let user = config.keypair.pubkey();
    let jito_tip = Pubkey::from_str("96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5").unwrap();
    let tip_lamports = (config.jito_tip * 1_000_000_000f64) as u64;

    println!("Main wallet: {}", user);
    println!("Jito tip: {} lamports", tip_lamports);

    let bh = rpc.get_latest_blockhash().await.unwrap();
    println!("Blockhash: {}", bh);

    // TX1: Jito tip (transfer to tip address)
    let tip_ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(3_000),
        system_instruction::transfer(&user, &jito_tip, tip_lamports),
    ];
    let tip_msg = Message::try_compile(&user, &tip_ixs, &[], bh).unwrap();
    let tip_tx = VersionedTransaction::try_new(VersionedMessage::V0(tip_msg), &[&config.keypair]).unwrap();
    let tip_encoded = encode(&tip_tx);

    // TX2: Transfer to a different Jito tip address (1 lamport, unique sig)
    let jito_tip2 = Pubkey::from_str("ADaUMid9yfUytqMBgopwjb2DTLSokTSzL1zt6iGPaS49").unwrap();
    let tx2_ixs = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(3_000),
        system_instruction::transfer(&user, &jito_tip2, 1),
    ];
    let tx2_msg = Message::try_compile(&user, &tx2_ixs, &[], bh).unwrap();
    let tx2 = VersionedTransaction::try_new(VersionedMessage::V0(tx2_msg), &[&config.keypair]).unwrap();
    let tx2_encoded = encode(&tx2);

    println!("\nTX1 (tip) encoded len: {} chars", tip_encoded.len());
    println!("TX2 (self) encoded len: {} chars", tx2_encoded.len());
    println!("TX1 first 20 chars: {}", &tip_encoded[..20]);
    println!("TX2 first 20 chars: {}", &tx2_encoded[..20]);
    println!("Same sigs? {}", tip_encoded == tx2_encoded);

    let bundle = vec![tip_encoded, tx2_encoded];

    println!("\nSending minimal 2-TX bundle to Jito...");
    // Use London/Singapore (currently available; NY/Amsterdam/Tokyo are rate limited)
    let client_http = reqwest::Client::new();
    let london_url = "https://london.mainnet.block-engine.jito.wtf/api/v1/bundles";
    let resp_result = client_http.post(london_url)
        .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"sendBundle","params":[bundle.clone()]}))
        .send().await;
    
    let bundle_id = match resp_result {
        Ok(r) => {
            let v: serde_json::Value = r.json().await.unwrap_or_default();
            if let Some(id) = v["result"].as_str() {
                println!("✅ Bundle sent (London): {}", id);
                id.to_string()
            } else {
                println!("❌ London error: {:?}", v);
                return;
            }
        }
        Err(e) => { println!("❌ Request error: {}", e); return; }
    };
    
    let id = bundle_id.clone();
    
    match Ok::<String, String>(id) {
        Ok(id) => {
            println!("✅ Bundle sent: {}", id);
            
            let client = reqwest::Client::new();
            
            // Check immediately (0s)
            for wait_secs in [0u64, 2, 5, 10] {
                if wait_secs > 0 {
                    tokio::time::sleep(tokio::time::Duration::from_secs(wait_secs)).await;
                }
                
                let inflight_raw = client.post("https://ny.mainnet.block-engine.jito.wtf/api/v1/bundles")
                    .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"getInflightBundleStatuses","params":[[id.clone()]]}))
                    .send().await;
                let inflight = if let Ok(r) = inflight_raw {
                    r.json::<serde_json::Value>().await.ok().map(|v| v["result"]["value"].clone())
                } else { None };
                
                let finalized_raw = client.post("https://ny.mainnet.block-engine.jito.wtf/api/v1/bundles")
                    .json(&serde_json::json!({"jsonrpc":"2.0","id":1,"method":"getBundleStatuses","params":[[id.clone()]]}))
                    .send().await;
                let finalized = if let Ok(r) = finalized_raw {
                    r.json::<serde_json::Value>().await.ok().map(|v| v["result"]["value"].clone())
                } else { None };
                
                let elapsed = wait_secs;
                println!("[+{}s] Inflight: {:?} | Finalized: {:?}", elapsed, inflight, finalized);
            }
        }
        Err(e) => println!("❌ Error: {}", e),
    }
}
