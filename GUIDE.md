# 📔 Advanced User Guide

This guide explains the "Magic" under the hood of the Slide-Fun Sniper.

---

## 🏗️ Architecture: How it Works

### The Dual-Listener Loop
The bot connects to Solana via **WebSockets**. It listens to two "streams" of data at once:

1.  **Slide.fun Stream**: 
    *   It waits for the `Migrate` instruction. 
    *   When it sees it, it immediately calculates the "Base Mint" (the token address) and creates an **ATA (Associated Token Account)** for your wallet. Doing this 1 second before everyone else saves precious milliseconds later.
2.  **Raydium Stream**: 
    *   It waits for `Initialize2` (the pool opening). 
    *   The moment it sees the pool, it checks if the token is from our Slide.fun list.
    *   If it matches, it fires the **Jito Bundle**.

---

## 🚀 Jito Bundles Explained

Normal transactions go into a "Mempool" and wait for a validator to pick them up. This is too slow for sniping.

**Jito** allows us to send a "Bundle" of transactions directly to a private validator. 
*   **Transaction 1**: Your Buy swap.
*   **Transaction 2**: A small tip to the Jito validator.

Because they are in a bundle, either **BOTH** happen or **NEITHER** happens. You never pay a tip if your buy fails.

---

## 🎛️ Understanding the Settings

| Setting | What it does | Recommended |
| :--- | :--- | :--- |
| `snipe_mode` | `raydium` (standard), `slidefun` (pre-migration), or `both`. | `raydium` |
| `jito_tip` | The bribe amount in SOL. | `0.003` to `0.01` |
| `cu_limit` | Compute Units. How much "brain power" the network uses for your swap. | `200,000` |
| `priority_fee` | Micro-lamports to prioritize the tx. | `100,000` |
| `dry_run` | If `true`, the bot simulates everything but doesn't spend SOL. | `true` (for testing) |

---

## 🎯 Targeted Snipe (Whitelist)

This is a powerful feature for "Called" tokens.
1.  Enter the token address in the **Target Mints** section of the dashboard.
2.  The bot will sit silently and **only** fire when that specific token migrates.
3.  This prevents you from accidentally buying 10 other tokens that happen to launch at the same time.

---

## ⚡ Manual Bundle Actions

Sometimes you want to snipe or sell an existing token across all your sub-wallets simultaneously, independent of the automated listeners.

1. **Manual Bundle Buy**: Paste the mint address and your desired SOL amount. The bot will automatically scrape the RPC for the token's AMM V4 pool address, construct buy transactions for *all* enabled sub-wallets, and fire them in a unified Jito bundle.
2. **Manual Bundle Sell**: Similar to buy, you provide a percentage (e.g. 50% or 100%). The bot will scan all sub-wallets for their token balances, create sell transactions, bundle them, and automatically unwrap the WSOL back into native SOL.

---

## 🧹 Maintenance & Troubleshooting

### "Address already in use" Error
This happens if you close the terminal but the bot is still running in the background. 
**Fix**: Run `lsof -ti:8080 | xargs kill -9` then restart.

### "401 Unauthorized"
Your Helius API key is wrong or has expired. Double check it in the dashboard.

### "Insufficient SOL"
Ensure your **Main Wallet** has at least **0.05 SOL** more than your `buy_amount` to cover the Jito tip and transaction fees.

---

## 💻 Developer Notes
If you want to modify the code:
*   `src/main.rs`: The entry point and orchestration.
*   `src/listener.rs`: The high-speed WebSocket logic.
*   `src/web.rs`: The Axum server for the dashboard.
*   `dashboard/index.html`: The UI code (HTML/CSS/JS).
