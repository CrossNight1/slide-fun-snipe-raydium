use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    signature::{Keypair, Signer},
    pubkey::Pubkey,
    transaction::Transaction,
    system_instruction,
};
use std::str::FromStr;

fn main() {
    let rpc = RpcClient::new("https://api.devnet.solana.com");
    // Main wallet keypair (from the config file or hardcoded for devnet testing)
    // Actually, any funded wallet can pay for it. I will generate a new one and airdrop it!
    let payer = Keypair::new();
    println!("Payer: {}", payer.pubkey());
    rpc.request_airdrop(&payer.pubkey(), 1_000_000_000).unwrap();
    std::thread::sleep(std::time::Duration::from_secs(5));
    
    // fee_to is the owner of the ATA AsNaB15WHEKhFjTH1KZXGQpyfZisA4rgQMUY2suU2AZg.
    // Let's just create an idempotent ATA for the fee_to. 
    // Wait, we don't know the fee_to address easily without reading the config account.
    // Let's read the config account: TB5B4zW99oy7F1mYfc5PaCoEQmZiCpExe4DRdh9E8n2
    let config_data = rpc.get_account_data(&Pubkey::from_str("TB5B4zW99oy7F1mYfc5PaCoEQmZiCpExe4DRdh9E8n2").unwrap()).unwrap();
    // In standard pumpfun, fee_recipient is at offset 8. pubkey is 32 bytes.
    let fee_to = Pubkey::new_from_array(config_data[8..40].try_into().unwrap());
    println!("fee_to: {}", fee_to);

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
