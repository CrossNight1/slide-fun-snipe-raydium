use solana_sdk::signature::Keypair;
use solana_sdk::signer::Signer;
use std::str::FromStr;

fn main() {
    let pk = "4kDtYG3iyAfq5tXf1Juy4EUds3UEQw35gBHLbjkwwgb39jFLJVPa8NLfk3DSXj8gjpLzqAUdQCWfUh1VcU6VzrWn";
    let bytes = bs58::decode(pk).into_vec().unwrap();
    let kp = Keypair::try_from(bytes.as_ref()).unwrap();
    println!("Main Wallet Pubkey: {}", kp.pubkey());
}
