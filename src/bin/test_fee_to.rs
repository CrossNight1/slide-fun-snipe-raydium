use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

#[tokio::main]
async fn main() {
    let rpc_client = RpcClient::new("https://api.devnet.solana.com".to_string());
    let program = Pubkey::from_str("BByYMVAn2jRJpy7Y5p8u2asobyG43TbWoNeaSTAZz8df").unwrap();

    let fee_to = slidefun_raydium_snipe::slidefun_snipe::fetch_fee_to(&rpc_client, &program).await;
    println!("Fee_to: {:?}", fee_to);
}
