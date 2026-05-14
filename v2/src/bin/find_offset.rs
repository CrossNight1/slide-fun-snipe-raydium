use solana_client::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

fn main() {
    let rpc = RpcClient::new("https://api.devnet.solana.com");
    let slidefun_program =
        Pubkey::from_str("6t4ZUwYAdpeB61y8jsMo4Kq1PZYaZvkfnaZUyXaJZeng").unwrap();
    let (config_pda, _) = Pubkey::find_program_address(&[b"config"], &slidefun_program);
    let data = rpc.get_account_data(&config_pda).unwrap();

    let expected = bs58::decode("4adRKEn4RsAL2L92PabxDLGgQicV8ifXRpCvD4aBbKYF")
        .into_vec()
        .unwrap();

    // Find index
    if let Some(pos) = data
        .windows(32)
        .position(|window| window == expected.as_slice())
    {
        println!("Found at offset: {}", pos);
    } else {
        println!("Not found in config data! Length of config: {}", data.len());
    }
}
