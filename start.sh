#!/bin/bash

# ==============================================================================
# Slide-Fun Raydium Sniper — Start Script
# ==============================================================================

set -e

# --- Colors ---
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# --- Banner ---
clear
echo -e "${BLUE}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║             SLIDE-FUN → RAYDIUM SNIPER BOT v0.2.0          ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════╝${NC}"

# --- Check for Rust ---
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}[ERROR] Rust/Cargo not found.${NC}"
    echo -e "Please install it from https://rustup.rs/ and restart your terminal."
    exit 1
fi

# --- Check config.json ---
if [ ! -f "config.json" ]; then
    echo -e "${YELLOW}[INFO] config.json not found. Creating from example...${NC}"
    if [ -f "config.json.example" ]; then
        cp config.json.example config.json
        echo -e "${GREEN}[OK] config.json created. PLEASE EDIT IT with your keys!${NC}"
    else
        echo -e "${RED}[ERROR] config.json.example not found. Cannot initialize config.${NC}"
        exit 1
    fi
fi

# --- Validate Helius Key ---
HELIUS_KEY=$(grep -o '"helius_api_key": "[^"]*"' config.json | cut -d'"' -f4)
if [[ "$HELIUS_KEY" == *"your_helius_api_key_here"* ]] || [[ -z "$HELIUS_KEY" ]]; then
    echo -e "${YELLOW}[WARNING] Helius API key is missing or default in config.json.${NC}"
    echo -e "You can configure it via the web dashboard or edit config.json manually."
fi

# --- Build ---
echo -e "${BLUE}[1/2] Building project (release mode)...${NC}"
cargo build --release

# --- Run ---
echo -e "${BLUE}[2/2] Starting sniper...${NC}"
echo -e "${GREEN}[OK] Bot is starting!${NC}"
echo -e "${YELLOW}------------------------------------------------------------${NC}"
echo -e "  🌐 WEB DASHBOARD: ${GREEN}http://localhost:8080${NC}"
echo -e "  📄 LOG FILE:      ${BLUE}slidefun_sniper.log${NC}"
echo -e "${YELLOW}------------------------------------------------------------${NC}"
echo ""

# Open the dashboard automatically (Mac/Linux)
(sleep 2 && open http://localhost:8080 || xdg-open http://localhost:8080) &> /dev/null &

# Run the binary in a loop to support "Force Restart" from the web dashboard
while true; do
    ./target/release/slidefun-raydium-snipe
    ./target/release/slidefun-raydium-snipe
    EXIT_CODE=$?
    
    # Restart on normal exit (code 0) or manual kill (e.g. 143)
    # Only stop if the user actually stops the script or it fails to find the binary
    if [ $EXIT_CODE -eq 130 ]; then
        echo -e "${BLUE}[INFO] Bot stopped by user (Ctrl+C).${NC}"
        exit 0
    fi

    echo -e "${YELLOW}[INFO] Bot stopped with code $EXIT_CODE. Restarting in 2 seconds...${NC}"
    sleep 2
done
