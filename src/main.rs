mod account;
mod actor;
mod ai;
mod app;
mod cache;
mod command;
mod config;
mod constants;
mod contacts;
mod credentials;
mod input;
mod mail;
#[cfg(feature = "notifications")]
mod notification;
mod oauth2;
mod ui;

use anyhow::Result;
use std::env;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::app::App;
use crate::config::Config;
use crate::credentials::CredentialStore;

fn setup_logging() {
    use std::fs::OpenOptions;

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug,bltz=debug"));

    // Try to create a log file in the config directory
    let log_file = Config::config_dir()
        .ok()
        .map(|dir| dir.join("bltz.log"))
        .and_then(|path| {
            OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .ok()
        });

    if let Some(file) = log_file {
        // Log to file
        tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(std::sync::Mutex::new(file))
                    .with_ansi(false),
            )
            .init();
    } else {
        // Fallback to stderr if file logging fails
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .init();
    }
}

fn print_usage() {
    eprintln!(
        r#"bltz - Fast terminal email client

Usage: bltz [command]

Commands:
    (none)      Start the email client
    setup       Configure email account and credentials
    help        Show this help message

Configuration file: ~/.config/bltz/config.toml
"#
    );
}

async fn run_setup() -> Result<()> {
    use std::io::{self, Write};

    println!("Bltz Setup");
    println!("=============\n");

    // Check if config exists
    let config_path = Config::config_path()?;
    if config_path.exists() {
        print!("Configuration already exists. Overwrite? [y/N]: ");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("Setup cancelled.");
            return Ok(());
        }
    }

    // Get email with validation
    let email = loop {
        print!("Email address: ");
        io::stdout().flush()?;
        let mut email = String::new();
        io::stdin().read_line(&mut email)?;
        let email = email.trim().to_string();

        // Basic email validation: must contain @ and have parts before/after
        if email.contains('@') {
            let parts: Vec<&str> = email.split('@').collect();
            if parts.len() == 2
                && !parts[0].is_empty()
                && parts[1].contains('.')
                && !parts[1].starts_with('.')
                && !parts[1].ends_with('.')
            {
                break email;
            }
        }
        println!(
            "Invalid email format. Please enter a valid email address (e.g., user@example.com)"
        );
    };

    // Get display name
    print!("Display name (optional): ");
    io::stdout().flush()?;
    let mut display_name = String::new();
    io::stdin().read_line(&mut display_name)?;
    let display_name = display_name.trim();
    let display_name = if display_name.is_empty() {
        None
    } else {
        Some(display_name.to_string())
    };

    // Get IMAP server with validation
    let imap_server = loop {
        print!("IMAP server: ");
        io::stdout().flush()?;
        let mut server = String::new();
        io::stdin().read_line(&mut server)?;
        let server = server.trim().to_string();

        // Basic hostname validation: alphanumeric with dots/hyphens, no leading/trailing dots
        if !server.is_empty()
            && server
                .chars()
                .all(|c| c.is_alphanumeric() || c == '.' || c == '-')
            && !server.starts_with('.')
            && !server.ends_with('.')
            && !server.starts_with('-')
            && server.contains('.')
        {
            break server;
        }
        println!("Invalid server hostname. Please enter a valid hostname (e.g., imap.example.com)");
    };

    // Get SMTP server with validation
    let smtp_server = loop {
        print!("SMTP server: ");
        io::stdout().flush()?;
        let mut server = String::new();
        io::stdin().read_line(&mut server)?;
        let server = server.trim().to_string();

        // Basic hostname validation
        if !server.is_empty()
            && server
                .chars()
                .all(|c| c.is_alphanumeric() || c == '.' || c == '-')
            && !server.starts_with('.')
            && !server.ends_with('.')
            && !server.starts_with('-')
            && server.contains('.')
        {
            break server;
        }
        println!("Invalid server hostname. Please enter a valid hostname (e.g., smtp.example.com)");
    };

    // Get password
    print!("Password: ");
    io::stdout().flush()?;
    let password = rpassword_read()?;
    println!();

    // Create config with new multi-account format
    let config = Config {
        accounts: vec![config::AccountConfig {
            email: email.clone(),
            username: None,
            display_name,
            imap: config::ImapConfig {
                server: imap_server,
                port: 993,
                tls: true,
            },
            smtp: config::SmtpConfig {
                server: smtp_server,
                port: 587,
                tls: true,
            },
            notifications: None,
            auth: config::AuthMethod::Password,
        }],
        default_account: Some(0),
        notifications: config::NotificationConfig::default(),
        ui: config::UiConfig::default(),
        cache: config::CacheConfig::default(),
        ai: config::AiConfig::default(),
    };

    // Save config
    config.ensure_dirs()?;
    config.save()?;
    println!("Configuration saved to {}", config_path.display());

    // Store password
    let creds = CredentialStore::new(&email);
    creds.set_password(&password)?;

    // Verify credentials were stored
    if creds.has_credentials() {
        println!("Password stored successfully.");
    } else {
        eprintln!("Warning: Failed to store credentials.");
        return Err(anyhow::anyhow!("Credential storage failed"));
    }

    println!("\nSetup complete! Run 'bltz' to start.");
    Ok(())
}

fn rpassword_read() -> Result<String> {
    use std::io;

    // Disable echo
    let _guard = DisableEcho::new()?;

    let mut password = String::new();
    io::stdin().read_line(&mut password)?;
    Ok(password.trim().to_string())
}

struct DisableEcho {
    #[cfg(unix)]
    original: libc::termios,
}

impl DisableEcho {
    #[cfg(unix)]
    fn new() -> Result<Self> {
        use std::mem::MaybeUninit;
        use std::os::unix::io::AsRawFd;

        let fd = std::io::stdin().as_raw_fd();
        let mut termios = MaybeUninit::<libc::termios>::uninit();

        unsafe {
            if libc::tcgetattr(fd, termios.as_mut_ptr()) != 0 {
                anyhow::bail!("Failed to get terminal attributes");
            }
            let original = termios.assume_init();
            let mut new = original;
            new.c_lflag &= !libc::ECHO;
            if libc::tcsetattr(fd, libc::TCSANOW, &new) != 0 {
                anyhow::bail!("Failed to set terminal attributes");
            }
            Ok(Self { original })
        }
    }

    #[cfg(not(unix))]
    fn new() -> Result<Self> {
        Ok(Self {})
    }
}

#[cfg(unix)]
impl Drop for DisableEcho {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        let fd = std::io::stdin().as_raw_fd();
        unsafe {
            libc::tcsetattr(fd, libc::TCSANOW, &self.original);
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    match args.get(1).map(|s| s.as_str()) {
        Some("help") | Some("--help") | Some("-h") => {
            print_usage();
            Ok(())
        }
        Some("setup") => run_setup().await,
        Some(cmd) => {
            eprintln!("Unknown command: {}", cmd);
            print_usage();
            std::process::exit(1);
        }
        None => {
            setup_logging();

            let config = Config::load()?;
            config.ensure_dirs()?;

            // Initialize theme from config
            crate::ui::theme::init_theme(config.ui.theme);

            // Get the default account
            let account = config.default_account().ok_or_else(|| {
                anyhow::anyhow!("No accounts configured. Run 'bltz setup' first.")
            })?;

            let creds = CredentialStore::new(&account.email);
            if !creds.has_credentials() {
                eprintln!("No credentials found for {}.", account.email);

                // Try to get more specific error info
                if let Err(e) = creds.get_imap_password() {
                    eprintln!("IMAP credential error: {}", e);
                }
                if let Err(e) = creds.get_smtp_password() {
                    eprintln!("SMTP credential error: {}", e);
                }

                eprintln!("\nPlease run 'bltz setup' to configure credentials.");
                eprintln!("Or set the BLTZ_PASSWORD environment variable:");
                eprintln!("  export BLTZ_PASSWORD='your-password'");
                eprintln!("  bltz");
                std::process::exit(1);
            }

            let mut app = App::new(config, creds).await?;
            app.run().await
        }
    }
}
