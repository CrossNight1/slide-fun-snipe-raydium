use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use slidefun_raydium_snipe::config::Config;
use slidefun_raydium_snipe::slidefun_snipe::fetch_fee_to;
use slidefun_raydium_snipe::bundle_buy::build_slidefun_buy_tx_for_wallet;
use slidefun_raydium_snipe::constants;
use std::str::FromStr;

#[tokio::main]
async fn main() {
    let rpc_client = RpcClient::new("https://api.devnet.solana.com".to_string());
    let program = Pubkey::from_str("BByYMVAn2jRJpy7Y5p8u2asobyG43TbWoNeaSTAZz8df").unwrap();

    let fee_to = fetch_fee_to(&rpc_client, &program).await.unwrap();
    let config = Config::from_env();
    
    // Use the main wallet
    let keypair = Keypair::from_bytes(&bs58::decode(&config.app.main_wallet.private_key).into_vec().unwrap()).unwrap();
    let token_mint = Pubkey::from_str("FLydCa69CMk7Gr3aBrquRCXBY8pHm2QcHx859a9NjW9s").unwrap();
    let token_program = Pubkey::from_str(constants::TOKEN_PROGRAM).unwrap();
    
    // 0.1 SOL
    let sol_lamports = 100_000_000;
    
    let blockhash = rpc_client.get_latest_blockhash().await.unwrap();

    let tx = build_slidefun_buy_tx_for_wallet(
        &keypair,
        &token_mint,
        &token_program,
        &fee_to,
        &program,
        sol_lamports,
        200_000,
        100_000,
        blockhash,
    ).unwrap();

    match rpc_client.simulate_transaction(&tx).await {
        Ok(res) => println!("Simulate: {:?}", res.value),
        Err(e) => println!("Error: {:?}", e),
    }
}
