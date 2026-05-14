use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_transaction_status_client_types::UiTransactionEncoding;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_transaction_status_client_types::option_serializer::OptionSerializer;
use std::str::FromStr;

#[tokio::main]
async fn main() {
    let rpc_client = RpcClient::new("https://api.devnet.solana.com".to_string());
    let sig_str = "4s9wEhv2TwbDXs47kz9fJqVma4G2Gq69udLCi9RhaTAZcT1ELtbNY5a4wkxRSWHR6nYSBLf3MPCJ71iG3a6khHRD";
    let sig = Signature::from_str(sig_str).unwrap();
    let program = Pubkey::from_str("HWy1jotHpo6UqeQxx49dpYYdQB8wj9Qk9MdxwjLvDHB8").unwrap();

    let config = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::Base64),
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(0),
    };

    if let Ok(tx) = rpc_client.get_transaction_with_config(&sig, config).await {
        if let Some(meta) = tx.transaction.meta {
            println!("Logs:");
            if let OptionSerializer::Some(logs) = meta.log_messages {
                for log in logs {
                    println!("{}", log);
                }
            }
        }
    }

    let pool = slidefun_raydium_snipe::pool::get_pool_info(&rpc_client, sig_str, program).await;
    println!("Pool: {:?}", pool);
}
