# 📖 Terminology & App Guide

Welcome to the **Slide-Fun Sniper** explanation guide. Here we clarify the key terms used throughout the application to help you trade with confidence.

---

### 🔑 Core Terms

#### **1. Mint Address**
The **Mint Address** is the unique identifier for a specific token on the Solana blockchain. It is equivalent to a "Contract Address" on Ethereum. 
*   *Why it matters:* This is what you paste into the app to tell it exactly which token to buy or sell.

#### **2. Target Mints (Whitelist)**
The **Whitelist** (or Target Mints) is a list of specific token addresses you want the bot to watch.
*   The bot will ignore all other tokens and **ONLY** fire when a token in this list graduates. Use this to focus on specific high-conviction launches.

#### **2.5 Auto Snipe All (Flag)**
*   *ON (True):* The bot ignores the Whitelist and automatically snipes **EVERY** token that graduates from Slide.fun to Raydium.
*   *OFF (False):* The bot only snipes tokens explicitly listed in your Whitelist. If the Whitelist is empty, it does nothing.

#### **3. Jito Tip**
A **Jito Tip** is a small bribe paid to validators who use the Jito-Solana client. 
*   *Why it matters:* It allows your transaction to be part of a "bundle" that is executed at the very beginning of a block, beating other traders who use standard public RPCs.

#### **4. Dry Run**
**Dry Run** is a "Safe Mode" for the bot.
*   *ON (True):* The bot performs all calculations, finds pools, and simulates the transaction on the network, but **DOES NOT** actually send your money.
*   *OFF (False):* The bot is LIVE and will spend real SOL. Always test in Dry Run first!

#### **5. Sub-Wallets (Bundles)**
These are additional wallets you control. When you "Bundle Buy," the bot fires multiple transactions (one for each wallet) in a single Jito bundle. 
*   *Benefit:* This allows you to accumulate a larger position across multiple accounts simultaneously without the market seeing multiple separate trades hitting the pool.

---

### ⚙️ Settings Explained

*   **Priority Fee:** Extra lamports paid to the network to get your transaction processed faster by standard validators.
*   **CU Limit (Compute Unit Limit):** Defines how much computational resources your transaction is allowed to use. 200k-300k is standard for Raydium swaps.
*   **WSOL (Wrapped SOL):** SOL wrapped into an SPL token format. Raydium uses WSOL for trading. The bot handles wrapping and unwrapping (closing the account) for you automatically.

---

### 🚀 Manual Actions

*   **Manual Buy:** Use this if a token is already live and you want to jump in with all your wallets at once.
*   **Manual Sell:** Allows you to exit your position (or a percentage of it) across all wallets in one click. The bot will also "Close" the WSOL accounts to reclaim the rent (roughly 0.002 SOL per wallet).
