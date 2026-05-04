# 🛠️ Quick Start Cheat Sheet

> For the full guide see `README.md`. This file is your day-to-day reference.

---

## First-time setup (run once)

```bash
# 1. Install Rust (skip if already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Enter the project
cd /Users/leoinv/Documents/CODE/slide-fun-snipe-raydium

# 3. Create config
cp .env.example .env
# → Edit .env: add PRIVATE_KEY and HELIUS_API_KEY

# 4. (Optional) Add sub-wallets
cp wallets.json.example wallets.json
# → Edit wallets.json: add private_key entries

# 5. Compile (takes 2-5 min the first time)
cargo build --release
```

---

## Run the bot

```bash
# Safe test (no real SOL sent) — DRY_RUN=true in .env
cargo run --release

# Go live — set DRY_RUN=false in .env first
cargo run --release
```

---

## Key `.env` settings to change before going live

| Setting | Safe Default | Live Value |
|---|---|---|
| `DRY_RUN` | `true` | **`false`** |
| `SNIPE_MODE` | `raydium` | `raydium` / `slidefun` / `both` |
| `SOL_AMOUNT` | `0.1` | Your desired buy size |
| `JITO_TIP` | `0.003` | Increase for more priority |

---

## Monitor logs

```bash
tail -f slidefun_sniper.log
```

---

## Utility tools

```bash
# Check your TX is under 1232 bytes
cargo run --bin measure_tx_size

# Test swap on an existing pool (uses your .env settings)
cargo run --bin test_swap -- <TX_SIGNATURE>
```
