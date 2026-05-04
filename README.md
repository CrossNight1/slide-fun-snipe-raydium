# 🚀 Slide-Fun → Raydium Sniper Bot

> A high-performance Rust bot that automatically buys tokens on **Slide.fun** and **Raydium** the moment they become available — using Jito bundle transactions for maximum on-chain priority.

---

## ⚡ How It Works

Slide.fun tokens go through three on-chain steps before trading on Raydium:

```
1. BondingCurveEnded  →  2. migrate()  →  3. createMarketAndPoolV4 (Raydium pool opens)
                              ↑                        ↑
                         [Listener A]            [Listener B]
                      Store token mint         Detect pool → FIRE SWAP
                      Pre-create ATA
```

The bot runs **two simultaneous WebSocket listeners** so it knows the token *before* the Raydium pool exists, giving it a timing advantage:

| Listener | Watches | Action |
|---|---|---|
| **A** (Slide.fun) | `migrate()` instruction | Saves token mint, pre-creates ATA |
| **B** (Raydium)   | `initialize2` instruction | Pool detected → immediate swap |

---

## 🗂️ Project Structure

```
src/
├── main.rs           ← Entry point: startup, config, orchestration
├── listener.rs       ← Dual WebSocket event loop (NEW — modular)
├── wallet.rs         ← WSOL pre-fund helper (NEW — modular)
├── handler.rs        ← Single-wallet Raydium swap TX builder
├── slidefun_snipe.rs ← Single-wallet Slide.fun bonding-curve snipe
├── bundle_buy.rs     ← Multi-wallet Jito bundle buy (up to 4 wallets/bundle)
├── graduation.rs     ← Detect migrate() + pre-create ATA
├── pool.rs           ← Parse Raydium pool data from transaction
├── transaction.rs    ← Swap instruction builder + Jito bundle submission
├── blockhash.rs      ← Background blockhash cache (400ms refresh)
├── config.rs         ← Load .env into typed Config struct
├── constants.rs      ← Program IDs, discriminators, Jito endpoints
├── types.rs          ← PoolInfo, GraduatingToken structs
└── logger.rs         ← Timestamped console + file logging
```

---

## 📋 Prerequisites

| Requirement | Notes |
|---|---|
| **Rust** ≥ 1.75 | Install via [rustup.rs](https://rustup.rs/) |
| **Helius API Key** | Free tier available at [helius.dev](https://helius.dev/) — needed for fast RPC |
| **Solana Wallet** | A funded mainnet wallet (private key in Base58 format) |
| **SOL Balance** | At least `SOL_AMOUNT + JITO_TIP + 0.01 SOL` for gas |

---

## 🛠️ Installation & Setup

### Step 1 — Clone and build

```bash
git clone <repo-url>
cd slide-fun-snipe-raydium
cargo build --release
```

> First build takes 2–5 minutes while Cargo downloads and compiles all dependencies.

### Step 2 — Create your `.env`

```bash
cp .env.example .env
```

Open `.env` in any text editor and fill in **at minimum**:

```env
PRIVATE_KEY=<your_base58_private_key>   # Your main wallet private key
HELIUS_API_KEY=<your_helius_api_key>    # From helius.dev
```

All other settings have safe defaults (see [Configuration Reference](#-configuration-reference) below).

### Step 3 — (Optional) Add sub-wallets for bundle buying

```bash
cp wallets.json.example wallets.json
```

Edit `wallets.json` and add the Base58 private keys of your sub-wallets:

```json
[
  {"private_key": "your_sub_wallet_1_base58"},
  {"private_key": "your_sub_wallet_2_base58"}
]
```

> Each sub-wallet needs at least `BUNDLE_SOL_PER_WALLET + 0.006 SOL` pre-loaded (covers the buy + ATA creation + gas).

---

## 🚦 Running the Bot

### Dry run (safe, no real SOL spent)

```bash
cargo run --release
```

With `DRY_RUN=true` (the default), the bot will listen and show you what transactions it *would* send, without spending any SOL. Use this to verify your setup is working.

### Go live

1. Edit your `.env` and set: `DRY_RUN=false`
2. Run: `cargo run --release`

---

## ⚙️ Configuration Reference

| Variable | Default | Description |
|---|---|---|
| `PRIVATE_KEY` | *(required)* | Main wallet Base58 private key |
| `HELIUS_API_KEY` | *(required)* | Helius RPC + WebSocket key |
| `SOL_AMOUNT` | `0.1` | SOL to spend per snipe (main wallet) |
| `CU_LIMIT` | `200000` | Compute unit limit for swap TX |
| `PRIORITY_FEE` | `100000` | Priority fee in µ-lamports |
| `JITO_TIP` | `0.003` | SOL tip sent to Jito validator per bundle |
| `DRY_RUN` | `true` | `true` = simulate only, no real trades |
| `TEST_MODE` | `false` | `true` = snipe ANY new AMM V4 pool (not just Slide.fun) |
| `SNIPE_MODE` | `raydium` | Snipe mode (see below) |
| `SLIDEFUN_PUMP_AMOUNT` | `0.05` | SOL per snipe in `slidefun` mode |
| `BUNDLE_WALLETS_FILE` | `wallets.json` | Path to sub-wallets file |
| `BUNDLE_SOL_PER_WALLET` | `0.05` | SOL each sub-wallet spends per bundle buy |
| `SLIDEFUN_PROGRAM` | *(hardcoded)* | Override Slide.fun program ID if it changes |

---

## 🎯 Snipe Modes

### `SNIPE_MODE=raydium` *(recommended, lower risk)*

Buys immediately when a Slide.fun-graduated token's **Raydium pool is created**.

```
Slide.fun  →  migrate()  →  [Bot detects, stores mint, pre-creates ATA]
                          →  Raydium pool appears  →  SWAP IMMEDIATELY
```

### `SNIPE_MODE=slidefun` *(higher speed, higher risk)*

Buys the moment a new token is **created on the Slide.fun bonding curve**, before it graduates.

```
Slide.fun CreateBondingCurve detected  →  BUY ON BONDING CURVE IMMEDIATELY
```

### `SNIPE_MODE=both`

Runs both strategies in a single process simultaneously.

| Mode | Where | When | Entry Price | Risk |
|---|---|---|---|---|
| `raydium` | Raydium AMM V4 | After graduation | Higher | Lower ✅ |
| `slidefun` | Slide.fun curve | At creation | Lowest | Higher ⚠️ |
| `both` | Both | Both moments | — | Mixed |

---

## 🔋 Multi-Wallet Bundle Buy

When `wallets.json` is populated, the bot fires **all wallets simultaneously** in Jito bundles:

```
[Main wallet tip TX]  ─┐
[Sub-wallet 1 buy  ]   ├─── Jito bundle (up to 5 TXs)
[Sub-wallet 2 buy  ]   │
[Sub-wallet 3 buy  ]   │
[Sub-wallet 4 buy  ]  ─┘

All bundles fired to 6 Jito regional endpoints in parallel
```

> **Tip**: Use 8–12 sub-wallets (2–3 bundles) to reduce Jito rate-limiting risk per slot.

---

## 🧪 Utility Commands

```bash
# Measure transaction byte size (no SOL needed)
cargo run --bin measure_tx_size

# Test a swap against an existing pool by TX signature
cargo run --bin test_swap -- <pool_init_tx_signature>

# Run a full bundle buy test (reads .env + wallets.json)
cargo run --bin test_bundle_buy
```

---

## 📄 Logs

The bot writes all output to both **stdout** and `slidefun_sniper.log` in the project directory. Each line is timestamped. You can monitor it with:

```bash
tail -f slidefun_sniper.log
```

---

## ⚠️ Security & Risk Warnings

> [!CAUTION]
> **Never share your `.env` or `wallets.json`** — they contain private keys that give full access to your wallets.

> [!WARNING]
> **Always test with `DRY_RUN=true` first.** Confirm the bot is connecting and detecting events correctly before going live.

> [!WARNING]
> **Sniping is competitive and risky.** Many tokens rug immediately after migration. You can lose your full snipe amount.

> [!NOTE]
> The `store` git credential helper saves your GitHub token in plain text at `~/.git-credentials`. This is fine for a personal dev machine but avoid it on shared servers.

---

## 📬 Key Addresses

| Name | Address |
|---|---|
| Slide.fun Program | `GkF6F9GNPjzkC18Xa3a88xwEc5vwyQDA1iXvFkKBqNDC` |
| Raydium AMM V4 | `675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8` |
| Raydium Authority | `5Q544fKrFoe6tsEbD7S8EmxGTJYAKtTVhAW5Q5pge4j1` |
