use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::Signature;
use std::str::FromStr;
use solana_transaction_status_client_types::{UiTransactionEncoding, option_serializer::OptionSerializer};

#[tokio::main]
async fn main() {
    let rpc = RpcClient::new("https://api.devnet.solana.com".to_string());
    let sig = Signature::from_str("2aJbq1owquKwHf7zSAWg62KKHgBZwNv6vYnjxpMTtF7SrfzPZrHVkQ2f6rdY2rJxvj162do8BJtKtxZ8dXDDqFNJ").unwrap();
    
    match rpc.get_transaction_with_config(
        &sig,
        solana_client::rpc_config::RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Json),
            commitment: Some(solana_sdk::commitment_config::CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        },
    ).await {
        Ok(tx) => {
            if let Some(meta) = tx.transaction.meta {
                println!("Error: {:?}", meta.err);
                if let OptionSerializer::Some(logs) = meta.log_messages {
                    println!("Logs:");
                    for log in logs {
                        println!("  {}", log);
                    }
                }
            }
        }
        Err(e) => println!("Error: {}", e),
    }
}
