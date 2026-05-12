use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 { return; }
    let rpc = RpcClient::new("https://api.devnet.solana.com".to_string());
    let pubkey = Pubkey::from_str(&args[1]).unwrap();
    match rpc.get_account(&pubkey).await {
        Ok(account) => println!("Owner: {}", account.owner),
        Err(e) => println!("Error: {}", e),
    }
}
