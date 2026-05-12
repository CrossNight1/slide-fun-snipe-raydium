use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::Arc;
use tokio::sync::Mutex;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trade {
    pub id: String, // Signature
    pub timestamp: DateTime<Utc>,
    pub mint: String,
    pub mode: String, // "slidefun" or "raydium"
    pub sol_amount: f64,
    pub token_amount: f64,
    pub status: String, // "success", "failed", "pending"
    pub latency_ms: Option<u64>,
    pub wallet: Option<String>,
    pub wallet_type: Option<String>, // "main" or "sub"
}

pub struct TradesStore {
    pub trades: Mutex<Vec<Trade>>,
    file_path: String,
}

impl TradesStore {
    pub fn new() -> Arc<Self> {
        let file_path = "trades.json".to_string();
        let trades = if let Ok(data) = fs::read_to_string(&file_path) {
            serde_json::from_str(&data).unwrap_or_else(|_| vec![])
        } else {
            vec![]
        };

        Arc::new(Self {
            trades: Mutex::new(trades),
            file_path,
        })
    }

    pub async fn add_trade(&self, trade: Trade) {
        let mut trades = self.trades.lock().await;
        // Dedup by signature — same TX can arrive from multiple RPC endpoints
        if trades.iter().any(|t| t.id == trade.id) {
            return;
        }
        trades.insert(0, trade); // Most recent first
        if trades.len() > 100 {
            trades.truncate(100); // Keep last 100
        }
        let _ = self.save(&trades);
    }

    pub async fn update_trade_status(&self, id: &str, status: &str, token_amount: f64) {
        let mut trades = self.trades.lock().await;
        if let Some(t) = trades.iter_mut().find(|t| t.id == id) {
            t.status = status.to_string();
            if token_amount > 0.0 {
                t.token_amount = token_amount;
            }
            let _ = self.save(&trades);
        }
    }

    pub async fn poll_pending_trades(&self, rpc_client: &solana_client::nonblocking::rpc_client::RpcClient) {
        let mut trades = self.trades.lock().await;
        let mut updated = false;

        for t in trades.iter_mut().filter(|t| t.status == "pending") {
            if let Ok(sig) = std::str::FromStr::from_str(&t.id) {
                match rpc_client.get_signature_status(&sig).await {
                    Ok(Some(Ok(()))) => {
                        t.status = "success".to_string();
                        updated = true;
                    }
                    Ok(Some(Err(_))) => {
                        t.status = "failed".to_string();
                        updated = true;
                    }
                    _ => {
                        // Still pending or not found yet.
                        // If it's been more than 60 seconds, mark as failed.
                        let age = chrono::Utc::now().signed_duration_since(t.timestamp).num_seconds();
                        if age > 60 {
                            t.status = "failed".to_string();
                            updated = true;
                        }
                    }
                }
            }
        }

        if updated {
            let _ = self.save(&trades);
        }
    }

    fn save(&self, trades: &[Trade]) -> std::io::Result<()> {
        let data = serde_json::to_string_pretty(trades).unwrap();
        fs::write(&self.file_path, data)
    }

    pub async fn get_all(&self) -> Vec<Trade> {
        self.trades.lock().await.clone()
    }

    pub async fn clear_trades(&self) {
        let mut trades = self.trades.lock().await;
        trades.clear();
        let _ = self.save(&trades);
    }
}
