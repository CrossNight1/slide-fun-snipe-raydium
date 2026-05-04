use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::hash::Hash;
use std::sync::Mutex;
use tokio::time::{sleep, Duration};
use crate::log_info;

lazy_static::lazy_static! {
    static ref CURRENT_BLOCKHASH: Mutex<Hash> = Mutex::new(Hash::default());
}

pub fn get_blockhash() -> Hash {
    *CURRENT_BLOCKHASH.lock().unwrap()
}

pub fn set_blockhash(hash: Hash) {
    *CURRENT_BLOCKHASH.lock().unwrap() = hash;
}

/// Background task: update blockhash every 50ms for ultra-fast TX building
pub async fn blockhash_updater(rpc_url: String) {
    let rpc_client = RpcClient::new(rpc_url);
    log_info!("[SYNC] Starting blockhash updater...");

    loop {
        match rpc_client.get_latest_blockhash().await {
            Ok(hash) => {
                set_blockhash(hash);
                sleep(Duration::from_millis(400)).await;
            }
            Err(e) => {
                log_info!("[WARN] Blockhash fetch error: {}", e);
                sleep(Duration::from_millis(200)).await;
            }
        }
    }
}
