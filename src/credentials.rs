use anyhow::Result;
use std::env;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

const ENV_PASSWORD: &str = "BLTZ_PASSWORD";

/// Debug information about credential storage backends
#[derive(Debug, Clone)]
pub struct CredentialDebugInfo {
    pub keyring_available: bool,
    pub env_var_set: bool,
    pub file_path: PathBuf,
    pub file_exists: bool,
}

impl std::fmt::Display for CredentialDebugInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Credential Storage Status:")?;
        writeln!(
            f,
            "  Keyring: {}",
            if self.keyring_available {
                "available"
            } else {
                "unavailable"
            }
        )?;
        writeln!(
            f,
            "  Environment var (BLTZ_PASSWORD): {}",
            if self.env_var_set { "set" } else { "not set" }
        )?;
        writeln!(f, "  File fallback: {}", self.file_path.display())?;
        writeln!(f, "  File exists: {}", self.file_exists)?;
        Ok(())
    }
}

pub struct CredentialStore {
    email: String,
    password_file: PathBuf,
}

impl CredentialStore {
    pub fn new(email: &str) -> Self {
        // Use email-specific password file to support multi-account
        let safe_email = email.replace(['@', '.', '/', '\\', ':'], "_");
        let password_file = crate::config::Config::config_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(format!(".password_{}", safe_email));

        Self {
            email: email.to_string(),
            password_file,
        }
    }

    /// Get diagnostic info about credential storage backend
    pub fn debug_info(&self) -> CredentialDebugInfo {
        // Check if keyring is available
        let test_key = format!("test:{}", self.email);
        let keyring_available = if let Ok(entry) = keyring::Entry::new("bltz", &test_key) {
            // Try a dummy operation to see if keyring works
            entry.set_password("__test__").is_ok()
                && entry.get_password().is_ok()
                && entry.delete_credential().is_ok()
        } else {
            false
        };

        let env_var_set = Self::env_password().is_some();
        let file_path = self.password_file.clone();
        let file_exists = self.password_file.exists();

        CredentialDebugInfo {
            keyring_available,
            env_var_set,
            file_path,
            file_exists,
        }
    }

    /// Check for password in environment variable first
    fn env_password() -> Option<String> {
        env::var(ENV_PASSWORD).ok()
    }

    /// Try to get password from keyring
    fn keyring_get(&self, key: &str) -> Option<String> {
        let entry = keyring::Entry::new("bltz", key).ok()?;
        entry.get_password().ok()
    }

    /// Try to set password in keyring
    fn keyring_set(&self, key: &str, password: &str) -> bool {
        if let Ok(entry) = keyring::Entry::new("bltz", key) {
            entry.set_password(password).is_ok()
        } else {
            false
        }
    }

    /// Read password from file fallback
    fn file_get(&self) -> Option<String> {
        fs::read_to_string(&self.password_file)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    /// Write password to file fallback (with restricted permissions)
    fn file_set(&self, password: &str) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.password_file.parent() {
            fs::create_dir_all(parent)?;
        }

        // Create file with restricted permissions atomically to avoid TOCTOU
        #[cfg(unix)]
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&self.password_file)?;
            file.write_all(password.as_bytes())?;
        }

        #[cfg(not(unix))]
        {
            fs::write(&self.password_file, password)?;
        }

        Ok(())
    }

    pub fn get_imap_password(&self) -> Result<String> {
        // 1. Try environment variable
        if let Some(pwd) = Self::env_password() {
            return Ok(pwd);
        }

        // 2. Try keyring
        let key = format!("imap:{}", self.email);
        if let Some(pwd) = self.keyring_get(&key) {
            return Ok(pwd);
        }

        // 3. Try file fallback
        if let Some(pwd) = self.file_get() {
            return Ok(pwd);
        }

        anyhow::bail!("Password not found. Set BLTZ_PASSWORD env var or run 'bltz setup'.")
    }

    pub fn get_smtp_password(&self) -> Result<String> {
        // 1. Try environment variable
        if let Some(pwd) = Self::env_password() {
            return Ok(pwd);
        }

        // 2. Try keyring
        let key = format!("smtp:{}", self.email);
        if let Some(pwd) = self.keyring_get(&key) {
            return Ok(pwd);
        }

        // 3. Try file fallback (same password for both)
        if let Some(pwd) = self.file_get() {
            return Ok(pwd);
        }

        anyhow::bail!("Password not found. Set BLTZ_PASSWORD env var or run 'bltz setup'.")
    }

    pub fn set_password(&self, password: &str) -> Result<()> {
        let imap_key = format!("imap:{}", self.email);
        let smtp_key = format!("smtp:{}", self.email);

        // Try keyring first
        let imap_ok = self.keyring_set(&imap_key, password);
        let smtp_ok = self.keyring_set(&smtp_key, password);

        if imap_ok && smtp_ok {
            // Verify it actually worked
            if self.keyring_get(&imap_key).is_some() && self.keyring_get(&smtp_key).is_some() {
                return Ok(());
            }
        }

        // Keyring failed, use file fallback
        eprintln!("Note: Keyring unavailable, using file-based storage.");
        self.file_set(password)?;

        Ok(())
    }

    pub fn has_credentials(&self) -> bool {
        // Environment variable
        if Self::env_password().is_some() {
            return true;
        }

        // Keyring
        let imap_key = format!("imap:{}", self.email);
        if self.keyring_get(&imap_key).is_some() {
            return true;
        }

        // File fallback
        if self.file_get().is_some() {
            return true;
        }

        false
    }

    #[allow(dead_code)]
    pub fn delete_all(&self) -> Result<()> {
        // Try to delete from keyring
        let imap_key = format!("imap:{}", self.email);
        let smtp_key = format!("smtp:{}", self.email);
        let oauth2_key = format!("oauth2:{}", self.email);

        if let Ok(entry) = keyring::Entry::new("bltz", &imap_key) {
            let _ = entry.delete_credential();
        }
        if let Ok(entry) = keyring::Entry::new("bltz", &smtp_key) {
            let _ = entry.delete_credential();
        }
        if let Ok(entry) = keyring::Entry::new("bltz", &oauth2_key) {
            let _ = entry.delete_credential();
        }

        // Delete file fallback
        let _ = fs::remove_file(&self.password_file);
        let _ = fs::remove_file(self.oauth2_token_file());

        Ok(())
    }

    // === OAuth2 Token Storage ===

    fn oauth2_token_file(&self) -> PathBuf {
        let safe_email = self.email.replace(['@', '.', '/', '\\', ':'], "_");
        crate::config::Config::config_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(format!(".oauth2_{}", safe_email))
    }

    /// Get OAuth2 refresh token
    pub fn get_oauth2_refresh_token(&self) -> Result<String> {
        // 1. Try keyring
        let key = format!("oauth2:{}", self.email);
        if let Some(token) = self.keyring_get(&key) {
            return Ok(token);
        }

        // 2. Try file fallback
        let token_file = self.oauth2_token_file();
        if let Ok(token) = fs::read_to_string(&token_file) {
            let token = token.trim().to_string();
            if !token.is_empty() {
                return Ok(token);
            }
        }

        anyhow::bail!("OAuth2 refresh token not found for {}", self.email)
    }

    /// Store OAuth2 refresh token
    pub fn set_oauth2_refresh_token(&self, refresh_token: &str) -> Result<()> {
        let key = format!("oauth2:{}", self.email);

        // Try keyring first
        if self.keyring_set(&key, refresh_token) {
            // Verify it worked
            if self.keyring_get(&key).is_some() {
                return Ok(());
            }
        }

        // Fall back to file
        let token_file = self.oauth2_token_file();

        // Ensure parent directory exists
        if let Some(parent) = token_file.parent() {
            fs::create_dir_all(parent)?;
        }

        // Create file with restricted permissions atomically to avoid TOCTOU
        #[cfg(unix)]
        {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&token_file)?;
            file.write_all(refresh_token.as_bytes())?;
        }

        #[cfg(not(unix))]
        {
            fs::write(&token_file, refresh_token)?;
        }

        Ok(())
    }

    /// Check if OAuth2 credentials are available
    #[allow(dead_code)]
    pub fn has_oauth2_credentials(&self) -> bool {
        let key = format!("oauth2:{}", self.email);
        if self.keyring_get(&key).is_some() {
            return true;
        }

        let token_file = self.oauth2_token_file();
        if let Ok(token) = fs::read_to_string(&token_file) {
            return !token.trim().is_empty();
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Mutex to prevent parallel test interference with env vars
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_env_password() {
        let _guard = ENV_MUTEX.lock().unwrap();
        env::set_var(ENV_PASSWORD, "test123");
        let store = CredentialStore::new("test@example.com");
        assert!(store.has_credentials());
        assert_eq!(store.get_imap_password().unwrap(), "test123");
        env::remove_var(ENV_PASSWORD);
    }

    #[test]
    fn test_email_specific_password_files() {
        // Verify that different emails get different password file paths
        let store1 = CredentialStore::new("user1@example.com");
        let store2 = CredentialStore::new("user2@example.com");

        assert_ne!(store1.password_file, store2.password_file);
        assert!(store1
            .password_file
            .to_string_lossy()
            .contains("user1_example_com"));
        assert!(store2
            .password_file
            .to_string_lossy()
            .contains("user2_example_com"));
    }

    #[test]
    fn test_special_chars_in_email_sanitized() {
        // Ensure special characters are sanitized in password file names
        let store = CredentialStore::new("user.name+tag@sub.domain.com");
        let filename = store.password_file.file_name().unwrap().to_string_lossy();

        // Filename should not contain any of these characters
        assert!(!filename.contains('@'), "filename contains @: {}", filename);
        assert!(!filename.contains('/'), "filename contains /: {}", filename);
        assert!(
            !filename.contains('\\'),
            "filename contains \\: {}",
            filename
        );
        assert!(!filename.contains(':'), "filename contains :: {}", filename);

        // Should be something like ".password_user_name+tag_sub_domain_com"
        assert!(
            filename.starts_with(".password_"),
            "unexpected filename: {}",
            filename
        );
    }

    #[test]
    fn test_file_fallback_isolation() {
        let _guard = ENV_MUTEX.lock().unwrap();
        // Ensure env var doesn't interfere
        env::remove_var(ENV_PASSWORD);

        // Create temp stores with unique emails for this test
        let email1 = format!("test_isolation_1_{}@example.com", std::process::id());
        let email2 = format!("test_isolation_2_{}@example.com", std::process::id());

        let store1 = CredentialStore::new(&email1);
        let store2 = CredentialStore::new(&email2);

        // Clean up any existing test files
        let _ = fs::remove_file(&store1.password_file);
        let _ = fs::remove_file(&store2.password_file);

        // Set password for account 1
        store1.file_set("password_for_account_1").unwrap();

        // Set password for account 2
        store2.file_set("password_for_account_2").unwrap();

        // Verify they don't interfere
        assert_eq!(
            store1.file_get(),
            Some("password_for_account_1".to_string())
        );
        assert_eq!(
            store2.file_get(),
            Some("password_for_account_2".to_string())
        );

        // Clean up
        let _ = fs::remove_file(&store1.password_file);
        let _ = fs::remove_file(&store2.password_file);
    }

    #[test]
    fn test_debug_info() {
        let store = CredentialStore::new("debug_test@example.com");
        let info = store.debug_info();

        // Should have a valid file path
        assert!(info
            .file_path
            .to_string_lossy()
            .contains("debug_test_example_com"));

        // Display should work
        let display = format!("{}", info);
        assert!(display.contains("Credential Storage Status:"));
        assert!(display.contains("Keyring:"));
        assert!(display.contains("File fallback:"));
    }

    #[test]
    fn test_env_takes_priority() {
        let _guard = ENV_MUTEX.lock().unwrap();

        let email = format!("priority_test_{}@example.com", std::process::id());
        let store = CredentialStore::new(&email);

        // Clean up
        let _ = fs::remove_file(&store.password_file);

        // Set file password
        store.file_set("file_password").unwrap();

        // Now set env var - it should take priority
        env::set_var(ENV_PASSWORD, "env_password");

        assert_eq!(store.get_imap_password().unwrap(), "env_password");
        assert_eq!(store.get_smtp_password().unwrap(), "env_password");

        // Clean up
        env::remove_var(ENV_PASSWORD);
        let _ = fs::remove_file(&store.password_file);
    }

    #[test]
    fn test_has_credentials_file_fallback() {
        let _guard = ENV_MUTEX.lock().unwrap();
        env::remove_var(ENV_PASSWORD);

        let email = format!("has_creds_test_{}@example.com", std::process::id());
        let store = CredentialStore::new(&email);

        // Clean up any existing file
        let _ = fs::remove_file(&store.password_file);

        // Should not have credentials initially
        // (keyring might or might not work, so we can't assert false here universally)

        // Set via file
        store.file_set("test_password").unwrap();

        // Now should have credentials
        assert!(store.has_credentials());

        // Should be able to retrieve
        // Note: This will only work if keyring also fails, otherwise keyring might not have it
        // So we test file_get directly
        assert_eq!(store.file_get(), Some("test_password".to_string()));

        // Clean up
        let _ = fs::remove_file(&store.password_file);
    }
}
