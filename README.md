# ⚡ Slide-Fun Sniper Bot (v0.2.0)

> **The ultimate Solana sniper for Slide.fun graduation.** 
> Built for speed, managed with ease via a beautiful web dashboard.

---

## 📖 What is this? (The Simple Explanation)

New tokens on Solana often start on **Slide.fun**. They are like "baby" tokens on a bonding curve. When enough people buy them, they "graduate" and move to **Raydium** (the big exchange).

**This bot's job is to be the first one to buy the token at the exact millisecond it arrives on Raydium.**

1.  **Listener A** watches Slide.fun. When a token graduates, it grabs the address.
2.  **Listener B** watches Raydium. The moment the trading pool opens, it fires a swap.
3.  **Jito Bundles**: It uses "bribes" (tips) to ensure your transaction is at the very top of the block.

---

## 🛠️ Quick Setup (Step-by-Step)

### 1. Preparation
*   **Install Rust**: If you don't have it, run: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
*   **Helius API Key**: Get a free key at [helius.dev](https://helius.dev). You need this to talk to the Solana network fast.

### 2. Initialization
Run the start script. It will create a `config.json` file for you:
```bash
./start.sh
```
*Wait for it to compile (the first time takes 2-5 minutes).*

### 3. Open the Dashboard
The bot will automatically open your browser to:
👉 **http://localhost:8080**

---

## 🌐 How to use the Dashboard

The dashboard is where you control everything. No more editing messy code!

### 👛 1. Setup your Wallets
*   **Main Wallet**: Paste your private key here. This wallet pays for the snipe and the Jito tip.
*   **Sub-wallets**: Add as many as you want! These will buy at the same time as your main wallet.
*   **Save**: Always click **Save Config** after making changes.

### ⚙️ 2. Global Settings
*   **SOL Amount**: How much SOL you want to spend per buy.
*   **Jito Tip**: The "bribe" amount. `0.003` is a good standard. Increase this during high competition.
*   **Dry Run**: **STAY ON TRUE** until you are ready. This lets you test without spending real SOL.

### 🎯 3. Target Mints (Whitelist)
*   **Empty List**: The bot snipes **EVERY** token that graduates from Slide.fun.
*   **Adding Addresses**: If you add a contract address (Mint Address) here, the bot will **IGNORE** everything else and **ONLY** snipe that specific token. Use this if you are following a "caller" or a specific launch.

### ⚡ 4. Manual Bundle Actions
*   **What is it?**: A tool to instantly buy or sell a specific token across *all* your wallets at the exact same time using Jito bundles.
*   **How to use**: Simply paste a **Token Mint Address**, choose how much you want to Buy (in SOL) or Sell (in %), and click the button. The bot will automatically find the Raydium pool and fire the transactions instantly.

---

## 🚦 Testing with No Money (Safe Mode)

Before risking real money, you should test the bot's speed and mechanics using **Dry Run** mode.

1.  **Enable Dry Run**: In the dashboard, make sure the **Dry Run** switch is set to **ON** (`DRY_RUN: true`), and click **Save Config**.
2.  **Run the Bot**: Run `./start.sh` in your terminal.
3.  **Watch the Logs**: Check the "Live Logs" in the dashboard. The bot will detect pools, build the transactions, and simulate them over the Solana network to see if they *would* have succeeded. 
4.  **No Money Spent**: The bot will skip sending the final bundle to Jito. Your SOL is 100% safe.
5.  **Go Live**: When you are confident, switch **Dry Run** to **OFF** (`DRY_RUN: false`), Save, and restart the bot. **Warning: This will spend real SOL.**

---

## ❓ Frequently Asked Questions

**Q: Why do I see a random wallet address on startup?**
A: If you haven't entered your private key yet, the bot makes a temporary one so it doesn't crash. Once you paste your real key in the dashboard and save, it will use yours.

**Q: What is "Test Mode"?**
A: **Keep this OFF** normally. If ON, the bot will buy *every* new pool on Raydium, even trash ones not from Slide.fun.

**Q: How do I stop the bot?**
A: Press `Ctrl + C` in your terminal window.

---

## ⚠️ Safety First
*   **Never share your `config.json`**: It contains your private keys.
*   **Keep enough SOL**: Your main wallet needs SOL for the buy + the tip + gas fees (~0.05 SOL extra is safe).
*   **Rugpulls**: Sniping is risky. Many tokens "rug" (the developers steal the money) immediately after migration. Only snipe what you can afford to lose.
