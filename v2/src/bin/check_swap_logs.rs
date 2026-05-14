use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::Signature;
use std::str::FromStr;
use solana_transaction_status_client_types::{UiTransactionEncoding, option_serializer::OptionSerializer};

#[tokio::main]
async fn main() {
    let rpc = RpcClient::new("https://api.devnet.solana.com".to_string());
    let sig = Signature::from_str("641H3VNvKG4xBHQ8Dm67YZepiwFreYaLx93BCkHBtFunTbgdqAF1XfgthQ7KEAxTp7Lcbo35HQB3AQqKBVjdPeNY").unwrap();
    
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
                if let OptionSerializer::Some(logs) = meta.log_messages {
                    for log in logs {
                        println!("{}", log);
                    }
                }
            }
        }
        Err(e) => println!("Error: {}", e),
    }
}
