// ==================== CONFIG ====================
//
// Unified configuration backed by `config.json`.
// Falls back to .env if config.json does not exist (backward compatibility).
//
// config.json structure (see config.json.example):
//   helius_api_key, snipe_mode, dry_run, test_mode,
//   jito_tip, cu_limit, priority_fee, slidefun_pump_amount,
//   main_wallet  { label, private_key, sol_amount, enabled }
//   bundle_wallets [ { label, private_key, sol_amount, enabled } ]

use crate::constants;
use serde::{Deserialize, Serialize};
use solana_sdk::{pubkey::Pubkey, signer::keypair::Keypair};
use std::{env, path::Path, str::FromStr};

pub const CONFIG_FILE: &str = "config.json";

// ─────────────────────────────────────────────
// Serializable config (stored in config.json)
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletEntry {
    pub label: String,
    pub private_key: String,
    #[serde(default = "default_sol_amount")]
    pub sol_amount: f64,
    #[serde(default = "default_manual_amount")]
    pub manual_sol_amount: f64,
    pub enabled: bool,
}

fn default_sol_amount() -> f64 {
    0.05
}
fn default_manual_amount() -> f64 {
    0.1
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_network")]
    pub network: String,
    pub helius_api_key: String,
    pub snipe_mode: String,
    pub dry_run: bool,
    pub test_mode: bool,
    pub jito_tip: f64,
    pub cu_limit: u32,
    pub priority_fee: u64,
    pub slidefun_pump_amount: f64,
    #[serde(alias = "slidefun_program")]
    pub slidefun_program_mainnet: Option<String>,
    pub slidefun_program_devnet: Option<String>,
    pub raydium_program_mainnet: Option<String>,
    pub raydium_program_devnet: Option<String>,
    pub mainnet_rpc: Option<String>,
    pub mainnet_ws: Option<String>,
    pub devnet_rpc: Option<String>,
    pub devnet_ws: Option<String>,
    #[serde(default = "default_jito_region")]
    pub jito_region: String,
    #[serde(default)]
    pub listen_creator: bool,
    #[serde(default)]
    pub auto_snipe_all: bool,
    #[serde(default)]
    pub target_mints: Vec<String>,
    #[serde(default)]
    pub target_mints_devnet: Vec<String>,
    pub target_creators: Vec<String>,
    #[serde(default)]
    pub target_creators_devnet: Vec<String>,
    pub main_wallet: WalletEntry,
    pub bundle_wallets: Vec<WalletEntry>,
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            network: "mainnet".to_string(),
            helius_api_key: String::new(),
            snipe_mode: "raydium".to_string(),
            dry_run: true,
            test_mode: false,
            jito_tip: 0.003,
            cu_limit: 200_000,
            priority_fee: 100_000,
            slidefun_pump_amount: 0.05,
            slidefun_program_mainnet: None,
            slidefun_program_devnet: None,
            raydium_program_mainnet: None,
            raydium_program_devnet: None,
            mainnet_rpc: None,
            mainnet_ws: None,
            devnet_rpc: None,
            devnet_ws: None,
            jito_region: "ny".to_string(),
            listen_creator: false,
            auto_snipe_all: false,
            target_mints: vec![],
            target_mints_devnet: vec![],
            target_creators: vec![],
            target_creators_devnet: vec![],
            main_wallet: WalletEntry {
                label: "Main Wallet".to_string(),
                private_key: "".to_string(),
                sol_amount: 0.05,
                manual_sol_amount: 0.1,
                enabled: true,
            },
            bundle_wallets: vec![],
        }
    }
}

impl AppConfig {
    /// Load from `config.json`. Falls back to `.env` if the file does not exist.
    pub fn load() -> Self {
        if Path::new(CONFIG_FILE).exists() {
            let content =
                std::fs::read_to_string(CONFIG_FILE).expect("[CONFIG] Failed to read config.json");
            serde_json::from_str(&content).expect("[CONFIG] Failed to parse config.json")
        } else {
            eprintln!("[CONFIG] config.json not found — falling back to .env");
            dotenvy::dotenv().ok();
            Self::from_env()
        }
    }

    /// Persist current config back to `config.json`.
    pub fn save(&self) -> Result<(), String> {
        let json =
            serde_json::to_string_pretty(self).map_err(|e| format!("Serialize error: {}", e))?;
        std::fs::write(CONFIG_FILE, json).map_err(|e| format!("Write error: {}", e))
    }

    /// Build from legacy .env environment variables.
    fn from_env() -> Self {
        AppConfig {
            network: env::var("SOLANA_NETWORK").unwrap_or_else(|_| "mainnet".to_string()),
            helius_api_key: env::var("HELIUS_API_KEY").unwrap_or_default(),
            snipe_mode: env::var("SNIPE_MODE")
                .unwrap_or_else(|_| "raydium".to_string())
                .to_lowercase(),
            dry_run: parse_bool(&env::var("DRY_RUN").unwrap_or_else(|_| "true".to_string())),
            test_mode: parse_bool(&env::var("TEST_MODE").unwrap_or_else(|_| "false".to_string())),
            jito_tip: env::var("JITO_TIP")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.003),
            cu_limit: env::var("CU_LIMIT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(200_000),
            priority_fee: env::var("PRIORITY_FEE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(100_000),
            slidefun_pump_amount: env::var("SLIDEFUN_PUMP_AMOUNT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.05),
            slidefun_program_mainnet: env::var("SLIDEFUN_PROGRAM").ok().filter(|v| !v.is_empty()),
            slidefun_program_devnet: env::var("SLIDEFUN_PROGRAM_DEVNET")
                .ok()
                .filter(|v| !v.is_empty()),
            raydium_program_mainnet: env::var("RAYDIUM_PROGRAM").ok().filter(|v| !v.is_empty()),
            raydium_program_devnet: env::var("RAYDIUM_PROGRAM_DEVNET")
                .ok()
                .filter(|v| !v.is_empty()),
            mainnet_rpc: env::var("MAINNET_RPC").ok().filter(|v| !v.is_empty()),
            mainnet_ws: env::var("MAINNET_WS").ok().filter(|v| !v.is_empty()),
            devnet_rpc: env::var("DEVNET_RPC").ok().filter(|v| !v.is_empty()),
            devnet_ws: env::var("DEVNET_WS").ok().filter(|v| !v.is_empty()),
            jito_region: env::var("JITO_REGION").unwrap_or_else(|_| "ny".to_string()),
            listen_creator: parse_bool(
                &env::var("LISTEN_CREATOR").unwrap_or_else(|_| "false".to_string()),
            ),
            auto_snipe_all: parse_bool(
                &env::var("AUTO_SNIPE_ALL").unwrap_or_else(|_| "false".to_string()),
            ),
            target_mints: vec![],
            target_mints_devnet: vec![],
            target_creators: vec![],
            target_creators_devnet: vec![],
            main_wallet: WalletEntry {
                label: "Main Wallet".to_string(),
                private_key: env::var("PRIVATE_KEY").unwrap_or_default(),
                sol_amount: env::var("MAIN_SOL_AMOUNT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.05),
                manual_sol_amount: env::var("MAIN_MANUAL_AMOUNT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.1),
                enabled: true,
            },
            bundle_wallets: load_bundle_wallets_from_file(
                &env::var("BUNDLE_WALLETS_FILE").unwrap_or_else(|_| "wallets.json".to_string()),
                env::var("BUNDLE_SOL_PER_WALLET")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0.05),
            ),
        }
    }
}

// ─────────────────────────────────────────────
// Runtime Config (with parsed Keypair objects)
// ─────────────────────────────────────────────

pub struct Config {
    /// The full serializable config (used for saving / web API)
    pub app: AppConfig,

    // --- convenience shortcuts for hot paths ---
    pub keypair: Keypair,
    pub network: String,
    pub helius_api_key: String,
    pub sol_amount: f64,
    pub cu_limit: u32,
    pub priority_fee: u64,
    pub jito_tip: f64,
    pub dry_run: bool,
    pub test_mode: bool,
    pub snipe_mode: String,
    pub slidefun_pump_amount: f64,
}

impl Config {
    pub fn from_app(app: AppConfig) -> Self {
        let keypair = match bs58::decode(&app.main_wallet.private_key).into_vec() {
            Ok(bytes) => match Keypair::try_from(bytes.as_ref()) {
                Ok(k) => k,
                Err(_) => {
                    if !app.main_wallet.private_key.is_empty()
                        && !app.main_wallet.private_key.contains("PASTE-")
                    {
                        eprintln!("[CONFIG] Error: Invalid Main Wallet Private Key bytes!");
                    }
                    Keypair::new()
                }
            },
            Err(_) => {
                if !app.main_wallet.private_key.is_empty()
                    && !app.main_wallet.private_key.contains("PASTE-")
                {
                    eprintln!("[CONFIG] Error: Invalid Main Wallet Private Key Base58!");
                }
                Keypair::new()
            }
        };
        Config {
            sol_amount: app.main_wallet.sol_amount,
            cu_limit: app.cu_limit,
            priority_fee: app.priority_fee,
            jito_tip: app.jito_tip,
            dry_run: app.dry_run,
            test_mode: app.test_mode,
            snipe_mode: app.snipe_mode.clone(),
            slidefun_pump_amount: app.slidefun_pump_amount,
            network: app.network.clone(),
            helius_api_key: app.helius_api_key.clone(),

            keypair,
            app,
        }
    }

    /// Load from environment / config.json.
    pub fn from_env() -> Self {
        Self::from_app(AppConfig::load())
    }

    /// Return only the *enabled* bundle wallets as (Keypair, sol_amount).
    pub fn enabled_bundle_keypairs(&self) -> Vec<(Keypair, f64)> {
        self.app
            .bundle_wallets
            .iter()
            .filter(|w| w.enabled && !w.private_key.is_empty())
            .filter_map(|w| {
                let bytes = bs58::decode(&w.private_key).into_vec().ok()?;
                let keypair = Keypair::try_from(bytes.as_ref()).ok()?;
                Some((keypair, w.sol_amount))
            })
            .collect()
    }

    /// Returns ALL enabled keypairs (Main + Sub-wallets) with their manual amounts.
    pub fn all_manual_keypairs(&self) -> Vec<(Keypair, f64)> {
        let mut results = vec![];

        // Main wallet
        let main_bytes = bs58::decode(&self.app.main_wallet.private_key)
            .into_vec()
            .unwrap_or_default();
        if let Ok(kp) = Keypair::try_from(main_bytes.as_ref()) {
            results.push((kp, self.app.main_wallet.manual_sol_amount));
        }

        // Sub-wallets
        for w in &self.app.bundle_wallets {
            if w.enabled {
                if let Ok(bytes) = bs58::decode(&w.private_key).into_vec() {
                    if let Ok(kp) = Keypair::try_from(bytes.as_ref()) {
                        results.push((kp, w.manual_sol_amount));
                    }
                }
            }
        }
        results
    }

    /// Check if a mint is in the target whitelist or if auto-snipe is enabled.
    pub fn is_whitelisted(&self, mint: &str) -> bool {
        if self.app.auto_snipe_all {
            return true;
        }
        if self.network.to_lowercase() == "devnet" {
            self.app.target_mints_devnet.iter().any(|m| m == mint)
        } else {
            self.app.target_mints.iter().any(|m| m == mint)
        }
    }

    /// Check if a creator is tracked (network-aware).
    pub fn is_creator_tracked(&self, creator: &str) -> bool {
        if self.network.to_lowercase() == "devnet" {
            self.app.target_creators_devnet.iter().any(|c| c == creator)
        } else {
            self.app.target_creators.iter().any(|c| c == creator)
        }
    }

    /// Get RPC URL (custom or Helius default)
    pub fn rpc_url(&self) -> String {
        if self.network.to_lowercase() == "devnet" {
            if let Some(url) = &self.app.devnet_rpc {
                if !url.is_empty() {
                    return url.clone();
                }
            }
            format!(
                "https://devnet.helius-rpc.com?api-key={}",
                self.helius_api_key
            )
        } else {
            if let Some(url) = &self.app.mainnet_rpc {
                if !url.is_empty() {
                    return url.clone();
                }
            }
            format!(
                "https://mainnet.helius-rpc.com?api-key={}",
                self.helius_api_key
            )
        }
    }

    /// Get WebSocket URL (custom or Helius default)
    pub fn ws_url(&self) -> String {
        if self.network.to_lowercase() == "devnet" {
            if let Some(url) = &self.app.devnet_ws {
                if !url.is_empty() {
                    return url.clone();
                }
            }
            format!(
                "wss://devnet.helius-rpc.com?api-key={}",
                self.helius_api_key
            )
        } else {
            if let Some(url) = &self.app.mainnet_ws {
                if !url.is_empty() {
                    return url.clone();
                }
            }
            format!(
                "wss://mainnet.helius-rpc.com?api-key={}",
                self.helius_api_key
            )
        }
    }

    /// Get Slide.fun program ID from config or constant.
    pub fn slidefun_program(&self) -> Pubkey {
        if self.network.to_lowercase() == "devnet" {
            if let Some(prog) = &self.app.slidefun_program_devnet {
                if !prog.is_empty() {
                    return Pubkey::from_str(prog).unwrap_or_else(|_| {
                        Pubkey::from_str(constants::SLIDEFUN_PROGRAM_DEVNET).unwrap()
                    });
                }
            }
            Pubkey::from_str(constants::SLIDEFUN_PROGRAM_DEVNET).unwrap()
        } else {
            if let Some(prog) = &self.app.slidefun_program_mainnet {
                if !prog.is_empty() {
                    return Pubkey::from_str(prog).unwrap_or_else(|_| {
                        Pubkey::from_str(constants::SLIDEFUN_PROGRAM).unwrap()
                    });
                }
            }
            Pubkey::from_str(constants::SLIDEFUN_PROGRAM).unwrap()
        }
    }

    /// Get Raydium program ID from config or constant.
    pub fn raydium_program(&self) -> Pubkey {
        if self.network.to_lowercase() == "devnet" {
            if let Some(prog) = &self.app.raydium_program_devnet {
                if !prog.is_empty() {
                    return Pubkey::from_str(prog).unwrap_or_else(|_| {
                        Pubkey::from_str(constants::RAYDIUM_AMM_PROGRAM_DEVNET).unwrap()
                    });
                }
            }
            Pubkey::from_str(constants::RAYDIUM_AMM_PROGRAM_DEVNET).unwrap()
        } else {
            if let Some(prog) = &self.app.raydium_program_mainnet {
                if !prog.is_empty() {
                    return Pubkey::from_str(prog).unwrap_or_else(|_| {
                        Pubkey::from_str(constants::RAYDIUM_AMM_PROGRAM).unwrap()
                    });
                }
            }
            Pubkey::from_str(constants::RAYDIUM_AMM_PROGRAM).unwrap()
        }
    }

    /// Get Slide.fun pump amount.
    pub fn slidefun_pump_amount(&self) -> f64 {
        self.app.slidefun_pump_amount
    }
}

// ─────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────

fn parse_bool(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "y" | "on"
    )
}

/// Load bundle wallets from the legacy wallets.json format.
fn load_bundle_wallets_from_file(path: &str, default_sol: f64) -> Vec<WalletEntry> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };
    let entries: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return vec![],
    };
    let mut wallets = Vec::new();
    if let Some(arr) = entries.as_array() {
        for (i, entry) in arr.iter().enumerate() {
            if let Some(pk) = entry["private_key"].as_str() {
                wallets.push(WalletEntry {
                    label: format!("Sub-wallet {}", i + 1),
                    private_key: pk.to_string(),
                    sol_amount: entry["sol_amount"].as_f64().unwrap_or(default_sol),
                    manual_sol_amount: entry["manual_sol_amount"].as_f64().unwrap_or(0.1),
                    enabled: true,
                });
            }
        }
    }
    wallets
}

fn default_network() -> String {
    "mainnet".to_string()
}

fn default_jito_region() -> String {
    "ny".to_string()
}
