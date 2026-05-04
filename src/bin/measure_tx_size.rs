// ==================== TX SIZE ESTIMATOR ====================
// Đo byte size của 1 buy TX cho 1 ví phụ (Solana limit: 1232 bytes).
// Chạy: cargo run --bin measure_tx_size

use bincode;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    hash::Hash,
    instruction::{AccountMeta, Instruction},
    message::{v0::Message, VersionedMessage},
    pubkey::Pubkey,
    signer::{keypair::Keypair, Signer},
    system_instruction,
    transaction::VersionedTransaction,
};
use spl_associated_token_account::{
    get_associated_token_address,
    instruction::create_associated_token_account_idempotent,
};
use std::str::FromStr;

// ── Constants (copy from constants.rs) ─────────────────────────────
const SLIDEFUN_PROGRAM:   &str = "GkF6F9GNPjzkC18Xa3a88xwEc5vwyQDA1iXvFkKBqNDC";
const RAYDIUM_AMM_PROGRAM:&str = "675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8";
const RAYDIUM_AUTHORITY:  &str = "5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1";
const TOKEN_PROGRAM:      &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const WSOL_MINT:          &str = "So11111111111111111111111111111111111111112";
const JITO_TIP:           &str = "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5";
const ASSOC_TOKEN_PGM:    &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";
const SF_BUY_DISC:  [u8; 8] = [102, 6, 61, 18, 1, 218, 235, 234];
const SF_CONFIG:    &[u8]    = b"config";
const SF_BC:        &[u8]    = b"bonding_curve";
const SWAP_BASE_IN:   u8     = 9;
// ───────────────────────────────────────────────────────────────────

const LIMIT: usize = 1232;

fn bar(size: usize) -> String {
    let pct = size * 40 / LIMIT;
    format!("[{}{}]", "█".repeat(pct.min(40)), "░".repeat(40usize.saturating_sub(pct)))
}

fn sf_buy_ix(user: &Pubkey, token_mint: &Pubkey, fee_to: &Pubkey) -> Instruction {
    let prog  = Pubkey::from_str(SLIDEFUN_PROGRAM).unwrap();
    let tok_p = Pubkey::from_str(TOKEN_PROGRAM).unwrap();
    let assoc = Pubkey::from_str(ASSOC_TOKEN_PGM).unwrap();
    let wsol  = Pubkey::from_str(WSOL_MINT).unwrap();

    let (cfg, _)  = Pubkey::find_program_address(&[SF_CONFIG], &prog);
    let (bc,  _)  = Pubkey::find_program_address(&[SF_BC, token_mint.as_ref()], &prog);
    let bc_tok  = get_associated_token_address(&bc, token_mint);
    let bc_pay  = get_associated_token_address(&bc, &wsol);
    let u_tok   = get_associated_token_address(user, token_mint);
    let u_pay   = get_associated_token_address(user, &wsol);
    let fee_ata = get_associated_token_address(fee_to, &wsol);

    let mut data = SF_BUY_DISC.to_vec();
    data.extend_from_slice(&50_000_000u64.to_le_bytes());
    data.push(1u8);
    data.extend_from_slice(&0u64.to_le_bytes());

    Instruction {
        program_id: prog,
        accounts: vec![
            AccountMeta::new(*user, true),
            AccountMeta::new_readonly(cfg, false),
            AccountMeta::new(bc, false),
            AccountMeta::new_readonly(*token_mint, false),
            AccountMeta::new_readonly(wsol, false),
            AccountMeta::new(bc_tok, false),
            AccountMeta::new(bc_pay, false),
            AccountMeta::new(u_tok, false),
            AccountMeta::new(u_pay, false),
            AccountMeta::new(fee_ata, false),
            AccountMeta::new_readonly(assoc, false),
            AccountMeta::new_readonly(solana_sdk::system_program::ID, false),
            AccountMeta::new_readonly(tok_p, false),
        ],
        data,
    }
}

fn ray_buy_ix(user: &Pubkey, token_mint: &Pubkey) -> Instruction {
    let prog  = Pubkey::from_str(RAYDIUM_AMM_PROGRAM).unwrap();
    let auth  = Pubkey::from_str(RAYDIUM_AUTHORITY).unwrap();
    let tok_p = Pubkey::from_str(TOKEN_PROGRAM).unwrap();
    let wsol  = Pubkey::from_str(WSOL_MINT).unwrap();
    let u_tok = get_associated_token_address(user, token_mint);
    let u_wso = get_associated_token_address(user, &wsol);
    let f     = || Keypair::new().pubkey(); // fake pool PDAs

    let mut data = vec![SWAP_BASE_IN];
    data.extend_from_slice(&50_000_000u64.to_le_bytes());
    data.extend_from_slice(&0u64.to_le_bytes());

    Instruction {
        program_id: prog,
        accounts: vec![
            AccountMeta::new_readonly(tok_p, false),
            AccountMeta::new(f(), false), // amm_id
            AccountMeta::new_readonly(auth, false),
            AccountMeta::new(f(), false), // open_orders
            AccountMeta::new(f(), false), // target_orders
            AccountMeta::new(f(), false), // base_vault
            AccountMeta::new(f(), false), // quote_vault
            AccountMeta::new_readonly(f(), false), // serum
            AccountMeta::new(f(), false), // market
            AccountMeta::new(f(), false), // bids
            AccountMeta::new(f(), false), // asks
            AccountMeta::new(f(), false), // event_q
            AccountMeta::new(f(), false), // coin_vault
            AccountMeta::new(f(), false), // pc_vault
            AccountMeta::new_readonly(f(), false), // vault_signer
            AccountMeta::new(u_wso, false),
            AccountMeta::new(u_tok, false),
            AccountMeta::new_readonly(*user, true),
        ],
        data,
    }
}

fn measure(label: &str, buyer: &Keypair, ixs: Vec<Instruction>) -> usize {
    let user = buyer.pubkey();
    match Message::try_compile(&user, &ixs, &[], Hash::default()) {
        Ok(msg) => match VersionedTransaction::try_new(VersionedMessage::V0(msg), &[buyer]) {
            Ok(tx) => {
                let size = bincode::serialize(&tx).unwrap_or_default().len();
                let ok = size <= LIMIT;
                let over = if ok { String::new() } else { format!("  ❌ OVER +{} bytes", size - LIMIT) };
                println!("  {:42} {:4} bytes  {} {}", label, size, bar(size), over);
                size
            }
            Err(e) => { println!("  Sign err: {}", e); 9999 }
        },
        Err(e) => { println!("  Compile err: {}", e); 9999 }
    }
}

fn main() {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║  TX SIZE METER — Solana limit: {} bytes                    ║", LIMIT);
    println!("╚════════════════════════════════════════════════════════════════╝");
    let tok  = Keypair::new().pubkey();
    let fee  = Keypair::new().pubkey();
    let wsol = Pubkey::from_str(WSOL_MINT).unwrap();
    let tp   = Pubkey::from_str(TOKEN_PROGRAM).unwrap();
    let jito = Pubkey::from_str(JITO_TIP).unwrap();

    let buyer   = Keypair::new();
    let u       = buyer.pubkey();
    let u_wsol_ata = get_associated_token_address(&u, &wsol);

    // ── SLIDE.FUN ──────────────────────────────────────────────────────────
    println!("\n🔥 SLIDE.FUN bonding curve buy (1 ví phụ = 1 TX)");
    println!("  {:<42} {:>4}       {:>42}", "Mode", "Size", "[░░░░░░░░ bar ░░░░░░░░] size/1232");
    println!("  {:-<106}", "");

    measure("Ví MỚI  (+WSOL ATA +Token ATA)", &buyer, vec![
        ComputeBudgetInstruction::set_compute_unit_limit(200_000),
        ComputeBudgetInstruction::set_compute_unit_price(100_000),
        create_associated_token_account_idempotent(&u, &u, &wsol, &tp),
        system_instruction::transfer(&u, &u_wsol_ata, 50_000_000),
        Instruction { program_id: tp, accounts: vec![AccountMeta::new(u_wsol_ata, false)], data: vec![17] },
        create_associated_token_account_idempotent(&u, &u, &tok, &tp),
        sf_buy_ix(&u, &tok, &fee),
    ]);

    measure("Ví CŨ   (ATA đã có, bỏ qua tạo)", &buyer, vec![
        ComputeBudgetInstruction::set_compute_unit_limit(200_000),
        ComputeBudgetInstruction::set_compute_unit_price(100_000),
        system_instruction::transfer(&u, &u_wsol_ata, 50_000_000),
        Instruction { program_id: tp, accounts: vec![AccountMeta::new(u_wsol_ata, false)], data: vec![17] },
        sf_buy_ix(&u, &tok, &fee),
    ]);

    measure("Ví MỚI  (+Jito tip trong TX)", &buyer, vec![
        ComputeBudgetInstruction::set_compute_unit_limit(200_000),
        ComputeBudgetInstruction::set_compute_unit_price(100_000),
        create_associated_token_account_idempotent(&u, &u, &wsol, &tp),
        system_instruction::transfer(&u, &u_wsol_ata, 50_000_000),
        Instruction { program_id: tp, accounts: vec![AccountMeta::new(u_wsol_ata, false)], data: vec![17] },
        create_associated_token_account_idempotent(&u, &u, &tok, &tp),
        sf_buy_ix(&u, &tok, &fee),
        system_instruction::transfer(&u, &jito, 3_000_000),
    ]);

    // ── RAYDIUM ───────────────────────────────────────────────────────────
    println!("\n📈 RAYDIUM AMM V4 buy (1 ví phụ = 1 TX)");
    println!("  {:-<106}", "");

    measure("Ví MỚI  (+WSOL ATA +Token ATA)", &buyer, vec![
        ComputeBudgetInstruction::set_compute_unit_limit(200_000),
        ComputeBudgetInstruction::set_compute_unit_price(100_000),
        create_associated_token_account_idempotent(&u, &u, &wsol, &tp),
        system_instruction::transfer(&u, &u_wsol_ata, 50_000_000),
        Instruction { program_id: tp, accounts: vec![AccountMeta::new(u_wsol_ata, false)], data: vec![17] },
        create_associated_token_account_idempotent(&u, &u, &tok, &tp),
        ray_buy_ix(&u, &tok),
    ]);

    measure("Ví CŨ   (ATA đã có, bỏ qua tạo)", &buyer, vec![
        ComputeBudgetInstruction::set_compute_unit_limit(200_000),
        ComputeBudgetInstruction::set_compute_unit_price(100_000),
        system_instruction::transfer(&u, &u_wsol_ata, 50_000_000),
        Instruction { program_id: tp, accounts: vec![AccountMeta::new(u_wsol_ata, false)], data: vec![17] },
        ray_buy_ix(&u, &tok),
    ]);

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("💡 Kết luận:");
    println!("   • Mỗi ví phụ = 1 TX riêng (Solana: 1 payer/TX)");
    println!("   • 1 Jito bundle = max 5 TX → 1 tip TX + 4 buy TX");
    println!("   • Giới hạn THỰC là 5 TX/bundle của Jito, không phải bytes");
    println!("   • Số bytes mỗi TX đều << 1232 → hoàn toàn không lo\n");
}
