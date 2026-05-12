use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::Signature;
use std::str::FromStr;
use solana_transaction_status_client_types::{UiTransactionEncoding, EncodedTransaction};

#[tokio::main]
async fn main() {
    let rpc = RpcClient::new("https://api.devnet.solana.com".to_string());
    let sig = Signature::from_str("2aJbq1owquKwHf7zSAWg62KKHgBZwNv6vYnjxpMTtF7SrfzPZrHVkQ2f6rdY2rJxvj162do8BJtKtxZ8dXDDqFNJ").unwrap();
    
    match rpc.get_transaction_with_config(
        &sig,
        solana_client::rpc_config::RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Base64),
            commitment: Some(solana_sdk::commitment_config::CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        },
    ).await {
        Ok(tx) => {
            if let Some(decoded) = tx.transaction.transaction.decode() {
                let message = &decoded.message;
                let keys = message.static_account_keys();
                println!("Account Keys:");
                for (i, key) in keys.iter().enumerate() {
                    println!("  [{}]: {}", i, key);
                }
                
                let instructions = match message {
                    solana_sdk::message::VersionedMessage::Legacy(m) => &m.instructions,
                    solana_sdk::message::VersionedMessage::V0(m) => &m.instructions,
                };
                
                for (i, ix) in instructions.iter().enumerate() {
                    let program_id = keys[ix.program_id_index as usize];
                    println!("Instruction {}: Program ID = {}", i, program_id);
                }
            } else {
                println!("Decode failed");
            }
        }
        Err(e) => println!("Error: {}", e),
    }
}
