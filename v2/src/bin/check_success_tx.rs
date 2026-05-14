use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::signature::Signature;
use std::str::FromStr;
use solana_transaction_status_client_types::{UiTransactionEncoding, EncodedTransaction};

#[tokio::main]
async fn main() {
    let rpc = RpcClient::new("https://api.devnet.solana.com".to_string());
    let sig = Signature::from_str("5zVS7W45hVXy8quDKN3TEUBcBFN4Sc2Uoq2EWE2Udf9G52WYw39Tm419dfzetRmfrRMFLoyYWiDMYYzJqpU4hkkf").unwrap();
    
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
                let instructions = match message {
                    solana_sdk::message::VersionedMessage::Legacy(m) => &m.instructions,
                    solana_sdk::message::VersionedMessage::V0(m) => &m.instructions,
                };
                for (i, ix) in instructions.iter().enumerate() {
                    let program_id = keys[ix.program_id_index as usize];
                    println!("Instruction {}: Program ID = {}", i, program_id);
                }
            }
        }
        Err(e) => println!("Error: {}", e),
    }
}
