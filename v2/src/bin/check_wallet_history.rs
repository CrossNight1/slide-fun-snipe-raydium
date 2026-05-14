use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use solana_client::rpc_client::GetConfirmedSignaturesForAddress2Config;

#[tokio::main]
async fn main() {
    let rpc = RpcClient::new("https://api.devnet.solana.com".to_string());
    let pubkey = Pubkey::from_str("EKdeMDvppCNA1YbwB46RtABzqFS6FFVNaXhCF6kNA8Rf").unwrap();
    
    match rpc.get_signatures_for_address_with_config(
        &pubkey,
        GetConfirmedSignaturesForAddress2Config {
            limit: Some(10),
            ..Default::default()
        }
    ).await {
        Ok(sigs) => {
            for sig_info in sigs {
                println!("Signature: {}", sig_info.signature);
                if let Some(err) = sig_info.err {
                    println!("  Error: {:?}", err);
                } else {
                    println!("  Status: Success");
                }
            }
        }
        Err(e) => println!("Error: {}", e),
    }
}
