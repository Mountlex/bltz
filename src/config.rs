use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Helper struct for parsing old single-account config format
#[derive(Debug, Clone, Deserialize)]
struct LegacyConfig {
    account: AccountConfig,
    #[serde(default)]
    ui: UiConfig,
    #[serde(default)]
    cache: CacheConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// List of email accounts (new format)
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
    /// Which account to use by default (index into accounts)
    #[serde(default)]
    pub default_account: Option<usize>,
    /// Desktop notification settings
    #[serde(default)]
    pub notifications: NotificationConfig,
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    /// AI features configuration (OpenRouter)
    #[serde(default)]
    pub ai: AiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    /// Enable desktop notifications for new mail
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Show email subject in notification
    #[serde(default = "default_true")]
    pub show_preview: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_preview: true,
        }
    }
}

/// Authentication method for an email account
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AuthMethod {
    /// Traditional password authentication
    #[default]
    Password,
    /// OAuth2 authentication (Gmail, etc.)
    OAuth2 {
        /// OAuth2 provider (e.g., "gmail")
        provider: String,
        /// OAuth2 client ID
        client_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub email: String,
    #[serde(default)]
    pub display_name: Option<String>,
    pub imap: ImapConfig,
    pub smtp: SmtpConfig,
    /// Per-account notification override (None = use global setting)
    #[serde(default)]
    pub notifications: Option<bool>,
    /// Authentication method (default: password)
    #[serde(default)]
    pub auth: AuthMethod,
}

impl AccountConfig {
    /// Get the display name or fall back to email
    pub fn display_name_or_email(&self) -> &str {
        self.display_name.as_deref().unwrap_or(&self.email)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImapConfig {
    pub server: String,
    #[serde(default = "default_imap_port")]
    pub port: u16,
    #[serde(default = "default_true")]
    pub tls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmtpConfig {
    pub server: String,
    #[serde(default = "default_smtp_port")]
    pub port: u16,
    #[serde(default = "default_true")]
    pub tls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default)]
    pub keybinding_mode: KeybindingMode,
    #[serde(default)]
    pub theme: ThemeVariant,
    #[serde(default = "default_date_format")]
    pub date_format: String,
    #[serde(default = "default_preview_length")]
    pub preview_length: usize,
    /// Split pane ratio for inbox view (30-70, default 50 = equal split)
    #[serde(default = "default_split_ratio")]
    pub split_ratio: u16,
    /// Show sent emails in inbox threads (conversation view)
    #[serde(default = "default_true")]
    pub conversation_mode: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum KeybindingMode {
    #[default]
    Vim,
    Arrows,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeVariant {
    #[default]
    Dark,
    #[serde(rename = "high-contrast")]
    HighContrast,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    #[serde(default = "default_max_cached_bodies")]
    pub max_cached_bodies: usize,
    #[serde(default = "default_sync_interval_secs")]
    pub sync_interval_secs: u64,
    /// Number of adjacent emails to prefetch in each direction (0 = disabled)
    #[serde(default = "default_prefetch_radius")]
    pub prefetch_radius: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_cached_bodies: default_max_cached_bodies(),
            sync_interval_secs: default_sync_interval_secs(),
            prefetch_radius: default_prefetch_radius(),
        }
    }
}

/// AI features configuration (OpenRouter integration)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// OpenRouter API key (required to enable AI features)
    #[serde(default)]
    pub api_key: Option<String>,
    /// Enable email summarization feature
    #[serde(default)]
    pub enable_summarization: bool,
    /// Enable grammar/writing polish feature
    #[serde(default)]
    pub enable_polish: bool,
    /// Model to use (default: anthropic/claude-3-haiku)
    #[serde(default = "default_ai_model")]
    pub model: String,
    /// Maximum tokens for summary responses
    #[serde(default = "default_summary_max_tokens")]
    pub summary_max_tokens: u32,
    /// Maximum tokens for polish responses
    #[serde(default = "default_polish_max_tokens")]
    pub polish_max_tokens: u32,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            enable_summarization: false,
            enable_polish: false,
            model: default_ai_model(),
            summary_max_tokens: default_summary_max_tokens(),
            polish_max_tokens: default_polish_max_tokens(),
        }
    }
}

impl AiConfig {
    /// Check if any AI features are enabled and configured
    pub fn is_enabled(&self) -> bool {
        self.api_key.is_some() && (self.enable_summarization || self.enable_polish)
    }
}

fn default_ai_model() -> String {
    "anthropic/claude-3-haiku".to_string()
}

fn default_summary_max_tokens() -> u32 {
    300
}

fn default_polish_max_tokens() -> u32 {
    2000
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            keybinding_mode: KeybindingMode::default(),
            theme: ThemeVariant::default(),
            date_format: default_date_format(),
            preview_length: default_preview_length(),
            split_ratio: default_split_ratio(),
            conversation_mode: true,
        }
    }
}

fn default_imap_port() -> u16 {
    993
}

fn default_smtp_port() -> u16 {
    587
}

fn default_true() -> bool {
    true
}

fn default_date_format() -> String {
    "%b %d".to_string()
}

fn default_preview_length() -> usize {
    100
}

fn default_max_cached_bodies() -> usize {
    500
}

fn default_split_ratio() -> u16 {
    50
}

fn default_sync_interval_secs() -> u64 {
    300
}

fn default_prefetch_radius() -> usize {
    2
}

impl Config {
    pub fn config_dir() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .context("Could not find config directory")?
            .join("bltz");
        Ok(dir)
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    pub fn data_dir() -> Result<PathBuf> {
        let dir = dirs::data_local_dir()
            .context("Could not find data directory")?
            .join("bltz");
        Ok(dir)
    }

    /// Get the default account (first account or the one specified by default_account)
    pub fn default_account(&self) -> Option<&AccountConfig> {
        if let Some(idx) = self.default_account {
            self.accounts.get(idx)
        } else {
            self.accounts.first()
        }
    }

    /// Get an account by index
    #[allow(dead_code)]
    pub fn account(&self, index: usize) -> Option<&AccountConfig> {
        self.accounts.get(index)
    }

    /// Get account by email address
    #[allow(dead_code)]
    pub fn account_by_email(&self, email: &str) -> Option<&AccountConfig> {
        self.accounts.iter().find(|a| a.email == email)
    }

    /// Check if notifications are enabled for an account
    pub fn notifications_enabled_for(&self, account: &AccountConfig) -> bool {
        // Per-account setting overrides global
        account.notifications.unwrap_or(self.notifications.enabled)
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;

        if !path.exists() {
            anyhow::bail!(
                "Configuration file not found at {}\n\
                 Please create a config file. Example:\n\n\
                 [[accounts]]\n\
                 email = \"you@example.com\"\n\n\
                 [accounts.imap]\n\
                 server = \"imap.example.com\"\n\n\
                 [accounts.smtp]\n\
                 server = \"smtp.example.com\"\n\n\
                 [ui]\n\
                 keybinding_mode = \"vim\"",
                path.display()
            );
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {}", path.display()))?;

        // Try parsing as new multi-account format first
        if let Ok(config) = toml::from_str::<Config>(&content)
            && !config.accounts.is_empty()
        {
            return Ok(config);
        }

        // Fall back to legacy single-account format
        let legacy: LegacyConfig = toml::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", path.display()))?;

        // Convert legacy config to new format
        Ok(Config {
            accounts: vec![legacy.account],
            default_account: Some(0),
            notifications: NotificationConfig::default(),
            ui: legacy.ui,
            cache: legacy.cache,
            ai: AiConfig::default(),
        })
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        let dir = path.parent().unwrap();

        fs::create_dir_all(dir)
            .with_context(|| format!("Failed to create config directory: {}", dir.display()))?;

        let content = toml::to_string_pretty(self).context("Failed to serialize config")?;

        fs::write(&path, content)
            .with_context(|| format!("Failed to write config file: {}", path.display()))?;

        Ok(())
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(Self::config_dir()?)?;
        fs::create_dir_all(Self::data_dir()?)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_new_multi_account_config() {
        let toml = r#"
            default_account = 0

            [[accounts]]
            email = "test@example.com"

            [accounts.imap]
            server = "imap.example.com"

            [accounts.smtp]
            server = "smtp.example.com"

            [[accounts]]
            email = "work@example.com"
            display_name = "Work"
            notifications = false

            [accounts.imap]
            server = "imap.work.com"

            [accounts.smtp]
            server = "smtp.work.com"

            [notifications]
            enabled = true
            show_preview = false

            [ui]
        "#;

        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.accounts.len(), 2);
        assert_eq!(config.accounts[0].email, "test@example.com");
        assert_eq!(config.accounts[1].email, "work@example.com");
        assert_eq!(config.accounts[1].display_name, Some("Work".to_string()));
        assert_eq!(config.accounts[1].notifications, Some(false));
        assert!(config.notifications.enabled);
        assert!(!config.notifications.show_preview);
        assert_eq!(config.ui.keybinding_mode, KeybindingMode::Vim);
    }

    #[test]
    fn test_parse_legacy_single_account_config() {
        // Test that old config format still works
        let toml = r#"
            [account]
            email = "test@example.com"

            [account.imap]
            server = "imap.example.com"

            [account.smtp]
            server = "smtp.example.com"

            [ui]
        "#;

        // Parse as legacy first
        let legacy: LegacyConfig = toml::from_str(toml).unwrap();
        assert_eq!(legacy.account.email, "test@example.com");
        assert_eq!(legacy.account.imap.port, 993);
        assert_eq!(legacy.account.smtp.port, 587);
    }

    #[test]
    fn test_default_account_helper() {
        let config = Config {
            accounts: vec![
                AccountConfig {
                    email: "first@example.com".to_string(),
                    display_name: None,
                    imap: ImapConfig {
                        server: "imap.example.com".to_string(),
                        port: 993,
                        tls: true,
                    },
                    smtp: SmtpConfig {
                        server: "smtp.example.com".to_string(),
                        port: 587,
                        tls: true,
                    },
                    notifications: None,
                    auth: AuthMethod::Password,
                },
                AccountConfig {
                    email: "second@example.com".to_string(),
                    display_name: Some("Second".to_string()),
                    imap: ImapConfig {
                        server: "imap2.example.com".to_string(),
                        port: 993,
                        tls: true,
                    },
                    smtp: SmtpConfig {
                        server: "smtp2.example.com".to_string(),
                        port: 587,
                        tls: true,
                    },
                    notifications: None,
                    auth: AuthMethod::Password,
                },
            ],
            default_account: Some(1),
            notifications: NotificationConfig::default(),
            ui: UiConfig::default(),
            cache: CacheConfig::default(),
            ai: AiConfig::default(),
        };

        // default_account is 1, so second account should be default
        assert_eq!(
            config.default_account().unwrap().email,
            "second@example.com"
        );
        assert_eq!(config.account(0).unwrap().email, "first@example.com");
        assert_eq!(
            config
                .account_by_email("second@example.com")
                .unwrap()
                .display_name,
            Some("Second".to_string())
        );
    }
}
