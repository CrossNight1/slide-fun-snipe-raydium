# 🧪 Slide.fun → Raydium Sniper Bot: Testing Guide

This guide will walk you through how to safely test the bot's features on the **Solana Devnet** before deploying your real capital on Mainnet.

## 📋 Phase 1: Preparation & Setup

### 1. Get Devnet SOL
To test transactions, you need Devnet SOL. 
1. Go to the [Solana Faucet](https://faucet.solana.com/).
2. Request Devnet SOL for your **Main Wallet** and all enabled **Sub-Wallets**.
3. *Optional:* Create a completely separate wallet to act as the "Creator" and fund it with Devnet SOL.

### 2. Configure the Bot for Devnet
Open your Dashboard (`http://localhost:8080`):
1. **Target Network**: Change to `Devnet`.
2. **Dry Run**: Turn this **OFF** if you want to execute real (devnet) trades. Leave it **ON** if you just want to see if the bot detects the events.
3. Click **Save All Settings**.

### 3. Setup Devnet Environment Variables
Devnet uses different program IDs than Mainnet. Create or update the `.env` file in the project folder:
```bash
# Slide.fun Devnet Program ID (Check Slide.fun docs or discord for their current Devnet ID)
SLIDEFUN_PROGRAM=GkF6F9GNPjzkC18Xa3a88xwEc5vwyQDA1iXvFkKBqNDC

# Raydium AMM V4 Devnet
RAYDIUM_AMM_PROGRAM=HWy90Zp86mN6p605C6U1Q8Jv5786W3K8092u2S8869S
RAYDIUM_AUTHORITY=DbQqP6ehDzyNsFkCuKkP2qXy8wz2GzKz9A3yR6E9oKk7

# Jito on Devnet
JITO_TIP_ADDRESS=96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5
JITO_BUNDLE_URLS=https://devnet.block-engine.jito.wtf/api/v1/bundles
```
*(Note: Jito bundles are not strictly supported on Devnet the same way as Mainnet. If transactions fail to land on Devnet, it is normal. Focus on ensuring the bot constructs and sends the transaction successfully.)*

---

## 🎯 Phase 2: Testing "Listen Creator" Mode

This mode allows you to snipe tokens the exact moment a specific wallet creates them on Slide.fun.

### Steps to Test:
1. **Get Creator Address**: Copy the public key of the separate "Creator" wallet you made in Phase 1.
2. **Dashboard Setup**: 
   - Add the Creator's public key to the **Target Creators** list.
   - Set the Snipe Mode to **Listen Creator**.
   - Start the bot (`./start.sh`). The bot will say it is listening.
3. **Create the Token**:
   - Go to the Devnet version of Slide.fun (or simulate a transaction calling the `CreateBondingCurve` instruction).
   - Use your Creator wallet to launch a new token.
4. **Observe the Bot**:
   - Immediately upon the transaction confirming on the blockchain, your bot logs should turn purple: `[SNIPE] 🎯 Creator matched! Sniping token...`.
   - The bot will execute bundled buys across all your configured sub-wallets.

---

## 🚀 Phase 3: Testing "Slide.fun" Mode (Bonding Curve)

This mode targets ANY new token launched on Slide.fun.

### Steps to Test:
1. **Dashboard Setup**:
   - Set Snipe Mode to **Slide.fun**.
   - If you want to snipe *everything*, turn **Auto Snipe All** to **ON**.
   - Start the bot.
2. **Action**:
   - Launch a token on Slide.fun Devnet from any wallet.
3. **Observe**:
   - The bot will instantly detect the `CreateBondingCurve` event globally.
   - It will bypass the whitelist checks (because Auto Snipe is ON) and execute the bundle buy immediately.

---

## 🎓 Phase 4: Testing "Raydium" Mode (Graduation)

This mode waits until a Slide.fun token finishes its bonding curve and is migrated to Raydium.

### Steps to Test:
1. **Dashboard Setup**:
   - Set Snipe Mode to **Raydium**.
   - Start the bot.
2. **Action**:
   - You need to simulate a graduation. Create a token on Slide.fun Devnet.
   - Using Devnet SOL, buy enough of the token on the Slide.fun bonding curve until it reaches 100% and triggers the migration.
3. **Observe**:
   - When the token migrates, the bot will detect the `Migrate` instruction from Slide.fun and add it to a watch-list.
   - Shortly after, Raydium will emit an `Initialize2` instruction.
   - The bot will detect this, match it against the watch-list, and fire the swap bundle.

---

## 🛠️ Codebase Health & Improvements

The codebase is currently very stable. However, before a production Mainnet deployment, consider the following improvements:

1. **Jito Tip Optimization**: Jito MEV tips are static right now. In a highly competitive Mainnet launch, you may want to implement dynamic Tip calculation based on current network congestion.
2. **WSOL Account Rent Reclamation**: The bot currently requires you to manually close WSOL accounts via the "Manual Sell" dashboard button. Adding an automated garbage collection task that runs once a day to reclaim rent could save SOL over time.
3. **Fallback RPCs**: If `mainnet.helius-rpc.com` goes down or rate-limits you, the bot will disconnect. Adding a secondary RPC url in `config.json` for automatic failover would increase uptime reliability.
