use serde_json::Value;
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::fs;
use std::str::FromStr;

fn main() {
    let rpc = RpcClient::new("https://api.devnet.solana.com");
    let config_str = fs::read_to_string("config.json").unwrap();
    let config_json: Value = serde_json::from_str(&config_str).unwrap();

    let pk_str = config_json["main_wallet"]["private_key"].as_str().unwrap();
    let pk_bytes = bs58::decode(pk_str).into_vec().unwrap();
    let payer = Keypair::from_bytes(&pk_bytes).unwrap();

    println!("Payer: {} (from config)", payer.pubkey());

    let slidefun_program =
        Pubkey::from_str("6t4ZUwYAdpeB61y8jsMo4Kq1PZYaZvkfnaZUyXaJZeng").unwrap();
    let (config_pda, _) = Pubkey::find_program_address(&[b"config"], &slidefun_program);
    println!("config_pda: {}", config_pda);

    let config_data = rpc.get_account_data(&config_pda).unwrap();
    let fee_to = Pubkey::new_from_array(config_data[80..112].try_into().unwrap());
    println!("REAL fee_to: {}", fee_to);

    let wsol = Pubkey::from_str("So11111111111111111111111111111111111111112").unwrap();
    let token_prog = Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA").unwrap();

    let ix = spl_associated_token_account::instruction::create_associated_token_account_idempotent(
        &payer.pubkey(),
        &fee_to,
        &wsol,
        &token_prog,
    );

    let recent_blockhash = rpc.get_latest_blockhash().unwrap();
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[&payer],
        recent_blockhash,
    );

    match rpc.send_and_confirm_transaction(&tx) {
        Ok(sig) => println!("Fixed Devnet! Sig: {}", sig),
        Err(e) => println!("Error fixing devnet: {}", e),
    }
}
