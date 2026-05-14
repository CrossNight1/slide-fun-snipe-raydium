use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

#[tokio::main]
async fn main() {
    let rpc_client = RpcClient::new("https://api.devnet.solana.com".to_string());
    let mint = Pubkey::from_str("FLydCa69CMk7Gr3aBrquRCXBY8pHm2QcHx859a9NjW9s").unwrap();

    let account = rpc_client.get_account(&mint).await.unwrap();
    println!("Owner: {:?}", account.owner);
}
