use crate::{
    constants::{JITO_BUNDLE_URLS, RAYDIUM_AMM_PROGRAM, RAYDIUM_AUTHORITY, SWAP_BASE_IN},
    log_info,
    types::PoolInfo,
};
use futures::stream::{FuturesUnordered, StreamExt};
use lazy_static::lazy_static;
use reqwest::Client;
use serde_json::{json, Value};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use std::str::FromStr;
use std::time::Duration;

lazy_static! {
    static ref JITO_HTTP_CLIENT: Client = Client::builder()
        .connect_timeout(Duration::from_millis(3000))
        .timeout(Duration::from_millis(8000))
        .pool_max_idle_per_host(16)
        .tcp_nodelay(true)
        .build()
        .expect("failed to build Jito HTTP client");
}

fn jito_region_name(url: &str) -> &str {
    url.split("://")
        .nth(1)
        .and_then(|s| s.split('.').next())
        .unwrap_or("unknown")
}

/// Send bundle to ALL Jito endpoints simultaneously for maximum speed
pub async fn send_via_jito(encoded_txs: &[String]) -> Result<String, Box<dyn std::error::Error>> {
    let mut inflight = FuturesUnordered::new();
    for url in JITO_BUNDLE_URLS {
        let url_str = url.to_string();
        let region = jito_region_name(url).to_string();
        let txs_clone = encoded_txs.to_vec();

        inflight.push(async move {
            let payload = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "sendBundle",
                "params": [txs_clone]
            });

            let response = JITO_HTTP_CLIENT
                .post(&url_str)
                .json(&payload)
                .send()
                .await
                .map_err(|e| format!("[{}] request error: {}", region, e))?;

            let result: Value = response
                .json()
                .await
                .map_err(|e| format!("[{}] decode error: {}", region, e))?;

            if let Some(bundle_id) = result["result"].as_str() {
                Ok::<(String, String), String>((region, bundle_id.to_string()))
            } else {
                Err(format!("[{}] Jito error: {:?}", region, result))
            }
        });
    }

    let mut errors: Vec<String> = Vec::new();
    let mut first_success: Option<String> = None;
    while let Some(result) = inflight.next().await {
        match result {
            Ok((region, bundle_id)) => {
                log_info!(
                    "[JITO] {} accepted bundle: {}",
                    region,
                    &bundle_id[..bundle_id.len().min(20)]
                );
                if first_success.is_none() {
                    first_success = Some(bundle_id);
                }
            }
            Err(e) => {
                log_info!("[JITO] {}", e);
                errors.push(e);
            }
        }
    }
    if let Some(bundle_id) = first_success {
        return Ok(bundle_id);
    }
    if errors.is_empty() {
        Err("All Jito endpoints failed".into())
    } else {
        Err(errors.join(" | ").into())
    }
}

/// Send a bundle to a single Jito Block Engine URL (used for per-region routing).
pub async fn send_bundle_to_url(
    encoded_txs: &[String],
    url: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let txs_clone = encoded_txs.to_vec();
    let payload = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendBundle",
        "params": [txs_clone]
    });

    let response = JITO_HTTP_CLIENT
        .post(url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("request error: {}", e).into()
        })?;

    let result: Value = response
        .json()
        .await
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
            format!("decode error: {}", e).into()
        })?;

    if let Some(bundle_id) = result["result"].as_str() {
        Ok(bundle_id.to_string())
    } else {
        Err(format!("Jito error: {:?}", result).into())
    }
}

/// Simulate a bundle against the first available Jito endpoint.
/// Returns the simulation result JSON for debugging.
pub async fn simulate_bundle_jito(encoded_txs: &[String]) -> Result<Value, Box<dyn std::error::Error>> {
    for url in JITO_BUNDLE_URLS {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "simulateBundle",
            "params": [
                { "encodedTransactions": encoded_txs },
                { "skipSigVerify": true, "replaceRecentBlockhash": true, "commitment": "confirmed" }
            ]
        });

        let response = match JITO_HTTP_CLIENT.post(*url).json(&payload).send().await {
            Ok(r) => r,
            Err(_) => continue,
        };

        let result: Value = match response.json().await {
            Ok(v) => v,
            Err(_) => continue,
        };

        if result.get("error").is_some() {
            let code = result["error"]["code"].as_i64().unwrap_or(0);
            if code == -32097 { continue; } // rate limited, try next endpoint
        }

        return Ok(result);
    }

    Err("All Jito endpoints failed or rate limited for simulateBundle".into())
}

/// Build Raydium AMM V4 swapBaseIn instruction
pub fn build_swap_instruction(
    pool_info: &PoolInfo,
    user: Pubkey,
    user_token_account: Pubkey,
    user_wsol_account: Pubkey,
    amount_in: u64,
    min_amount_out: u64,
    token_program: Pubkey,
) -> Instruction {
    let raydium_program = Pubkey::from_str(RAYDIUM_AMM_PROGRAM).unwrap();
    let raydium_authority = Pubkey::from_str(RAYDIUM_AUTHORITY).unwrap();

    let accounts = vec![
        AccountMeta::new_readonly(token_program, false),
        AccountMeta::new(pool_info.amm_id, false),
        AccountMeta::new_readonly(raydium_authority, false),
        AccountMeta::new(pool_info.open_orders, false),
        AccountMeta::new(pool_info.target_orders, false),
        AccountMeta::new(pool_info.base_vault, false),
        AccountMeta::new(pool_info.quote_vault, false),
        AccountMeta::new_readonly(pool_info.serum_program_id, false),
        AccountMeta::new(pool_info.market_id, false),
        AccountMeta::new(pool_info.market_bids, false),
        AccountMeta::new(pool_info.market_asks, false),
        AccountMeta::new(pool_info.market_event_queue, false),
        AccountMeta::new(pool_info.market_coin_vault, false),
        AccountMeta::new(pool_info.market_pc_vault, false),
        AccountMeta::new_readonly(pool_info.market_vault_signer, false),
        AccountMeta::new(user_wsol_account, false),
        AccountMeta::new(user_token_account, false),
        AccountMeta::new_readonly(user, true),
    ];

    let mut data = vec![SWAP_BASE_IN];
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_amount_out.to_le_bytes());

    Instruction {
        program_id: raydium_program,
        accounts,
        data,
    }
}
