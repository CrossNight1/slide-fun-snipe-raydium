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

use serde::{Deserialize, Serialize};
use solana_sdk::signer::keypair::Keypair;
use std::{env, path::Path};

pub const CONFIG_FILE: &str = "config.json";

// ─────────────────────────────────────────────
// Serializable config (stored in config.json)
// ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletEntry {
    pub label: String,
    pub private_key: String,
    pub sol_amount: f64,
    pub enabled: bool,
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
    pub slidefun_program: Option<String>,
    #[serde(default)]
    pub auto_snipe_all: bool,
    #[serde(default)]
    pub target_mints: Vec<String>,
    #[serde(default)]
    pub target_creators: Vec<String>,
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
            slidefun_program: None,
            auto_snipe_all: false,
            target_mints: vec![],
            target_creators: vec![],
            main_wallet: WalletEntry {
                label: "Main Wallet".to_string(),
                private_key: String::new(),
                sol_amount: 0.1,
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
            let content = std::fs::read_to_string(CONFIG_FILE)
                .expect("[CONFIG] Failed to read config.json");
            serde_json::from_str(&content).expect("[CONFIG] Failed to parse config.json")
        } else {
            eprintln!("[CONFIG] config.json not found — falling back to .env");
            dotenvy::dotenv().ok();
            Self::from_env()
        }
    }

    /// Persist current config back to `config.json`.
    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Serialize error: {}", e))?;
        std::fs::write(CONFIG_FILE, json).map_err(|e| format!("Write error: {}", e))
    }

    /// Build from legacy .env environment variables.
    fn from_env() -> Self {
        AppConfig {
            network: env::var("SOLANA_NETWORK").unwrap_or_else(|_| "mainnet".to_string()),
            helius_api_key: env::var("HELIUS_API_KEY")
                .unwrap_or_default(),
            snipe_mode: env::var("SNIPE_MODE")
                .unwrap_or_else(|_| "raydium".to_string())
                .to_lowercase(),
            dry_run: parse_bool(&env::var("DRY_RUN").unwrap_or_else(|_| "true".to_string())),
            test_mode: parse_bool(&env::var("TEST_MODE").unwrap_or_else(|_| "false".to_string())),
            jito_tip: env::var("JITO_TIP").ok().and_then(|v| v.parse().ok()).unwrap_or(0.003),
            cu_limit: env::var("CU_LIMIT").ok().and_then(|v| v.parse().ok()).unwrap_or(200_000),
            priority_fee: env::var("PRIORITY_FEE").ok().and_then(|v| v.parse().ok()).unwrap_or(100_000),
            slidefun_pump_amount: env::var("SLIDEFUN_PUMP_AMOUNT").ok().and_then(|v| v.parse().ok()).unwrap_or(0.05),
            slidefun_program: env::var("SLIDEFUN_PROGRAM").ok().filter(|v| !v.is_empty()),
            auto_snipe_all: parse_bool(&env::var("AUTO_SNIPE_ALL").unwrap_or_else(|_| "false".to_string())),
            target_mints: vec![],
            target_creators: vec![],
            main_wallet: WalletEntry {
                label: "Main Wallet".to_string(),
                private_key: env::var("PRIVATE_KEY").unwrap_or_default(),
                sol_amount: env::var("SOL_AMOUNT").ok().and_then(|v| v.parse().ok()).unwrap_or(0.1),
                enabled: true,
            },
            bundle_wallets: load_bundle_wallets_from_file(
                &env::var("BUNDLE_WALLETS_FILE").unwrap_or_else(|_| "wallets.json".to_string()),
                env::var("BUNDLE_SOL_PER_WALLET").ok().and_then(|v| v.parse().ok()).unwrap_or(0.05),
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
    pub bundle_wallets_file: String, // kept for compat, unused in JSON mode
    pub bundle_sol_per_wallet: f64,
}

impl Config {
    pub fn from_app(app: AppConfig) -> Self {
        let keypair = match bs58::decode(&app.main_wallet.private_key).into_vec() {
            Ok(bytes) => match Keypair::from_bytes(&bytes) {
                Ok(k) => k,
                Err(_) => {
                    if !app.main_wallet.private_key.is_empty() && !app.main_wallet.private_key.contains("PASTE-") {
                        eprintln!("[CONFIG] Error: Invalid Main Wallet Private Key bytes!");
                    }
                    Keypair::new()
                }
            },
            Err(_) => {
                if !app.main_wallet.private_key.is_empty() && !app.main_wallet.private_key.contains("PASTE-") {
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
            bundle_wallets_file: "wallets.json".to_string(),
            bundle_sol_per_wallet: app.bundle_wallets
                .first()
                .map(|w| w.sol_amount)
                .unwrap_or(0.05),
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


    /// Check if a mint is in the target whitelist or if auto-snipe is enabled.
    pub fn is_whitelisted(&self, mint: &str) -> bool {
        if self.app.auto_snipe_all {
            return true;
        }
        self.app.target_mints.iter().any(|m| m == mint)
    }
}

// ─────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────

fn parse_bool(s: &str) -> bool {
    matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "y" | "on")
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
