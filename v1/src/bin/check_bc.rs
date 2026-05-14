use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use slidefun_raydium_snipe::slidefun_snipe::derive_bonding_curve_pda;

#[tokio::main]
async fn main() {
    let rpc_client = RpcClient::new("https://api.devnet.solana.com".to_string());
    let program = Pubkey::from_str("BByYMVAn2jRJpy7Y5p8u2asobyG43TbWoNeaSTAZz8df").unwrap();
    let mint = Pubkey::from_str("FLydCa69CMk7Gr3aBrquRCXBY8pHm2QcHx859a9NjW9s").unwrap();

    let bc_pda = derive_bonding_curve_pda(&mint, &program);
    println!("Bonding Curve PDA: {}", bc_pda);

    match rpc_client.get_account(&bc_pda).await {
        Ok(acc) => {
            println!("Bonding curve exists! Data len: {}", acc.data.len());
        }
        Err(e) => {
            println!("Error fetching bonding curve: {:?}", e);
        }
    }
}
