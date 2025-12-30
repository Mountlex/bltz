//! Application core - manages state, accounts, and coordination

mod actions;
mod event_loop;
pub mod undo;

use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::collections::HashSet;
use std::io;
use std::sync::Arc;
use std::time::Instant;

use crate::account::AccountManager;
use crate::ai::{spawn_ai_actor, AiActorHandle, OpenRouterClient};
use crate::cache::Cache;
use crate::config::Config;
use crate::constants::{EMAIL_PAGE_SIZE, PREFETCH_DEBOUNCE_MS};
use crate::contacts::ContactsDb;
use crate::credentials::CredentialStore;
use crate::input::KeyBindings;
use crate::mail::{folder_cache_key, group_into_threads};
use crate::ui::app::AppState;

use self::undo::{PendingDeletion, UndoEntry};

pub struct App {
    pub(crate) config: Config,
    pub(crate) cache: Arc<Cache>,
    pub(crate) contacts: ContactsDb,
    pub(crate) accounts: AccountManager,
    pub(crate) state: AppState,
    pub(crate) bindings: KeyBindings,
    /// Track last prefetch UID to avoid redundant requests
    pub(crate) last_prefetch_uid: Option<u32>,
    /// Pending prefetch: (uids, when_requested) - for debouncing rapid navigation
    /// First UID is the current selection; remaining are nearby emails
    pub(crate) pending_prefetch: Option<(Vec<u32>, Instant)>,
    /// UIDs currently being fetched (prevents duplicate requests)
    pub(crate) in_flight_fetches: HashSet<u32>,
    /// Whether we've done initial folder prefetch (after first INBOX sync)
    pub(crate) prefetch_done: bool,
    /// Stack of undoable actions (most recent first)
    pub(crate) undo_stack: Vec<UndoEntry>,
    /// Pending deletions waiting to be executed (delayed by 10 seconds)
    pub(crate) pending_deletions: Vec<PendingDeletion>,
    /// Tracks when the last search input was received (for debouncing body FTS)
    pub(crate) last_search_input: Option<Instant>,
    /// AI actor handle for summarization and polish (None if AI features disabled)
    pub(crate) ai_actor: Option<AiActorHandle>,
}

impl App {
    pub async fn new(config: Config, _credentials: CredentialStore) -> Result<Self> {
        // Open cache database (async)
        let cache_path = Config::data_dir()?.join("cache.db");
        let cache = Arc::new(Cache::open(&cache_path).await?);

        // Create contacts DB (shares connection path with cache)
        let contacts = ContactsDb::open(&cache_path).await?;

        // Create account manager (spawns IMAP actors for all accounts)
        let accounts = AccountManager::new(&config, Arc::clone(&cache)).await?;

        // Get the default account for initial state
        let account_id = accounts.active().account_id.clone();
        let account_name = accounts.active().display_name().to_string();

        // Create keybindings
        let bindings = KeyBindings::new(&config.ui.keybinding_mode);

        // Default folder is INBOX
        let default_folder = "INBOX".to_string();
        let cache_key = folder_cache_key(&account_id, &default_folder);

        // Load initial state from cache (for default folder)
        let emails = cache.get_emails(&cache_key, EMAIL_PAGE_SIZE, 0).await?;
        let emails_loaded = emails.len();
        let threads = group_into_threads(&emails);
        let total_count = cache.get_email_count(&cache_key).await?;
        let unread_count = cache.get_unread_count(&cache_key).await?;

        // Build account names list for composer
        let account_names: Vec<String> = accounts
            .iter()
            .map(|h| h.display_name().to_string())
            .collect();

        let state = AppState {
            emails,
            threads,
            total_count,
            unread_count,
            loading: true, // Actor is connecting
            split_ratio: config.ui.split_ratio.clamp(30, 70),
            account_name,
            account_index: accounts.active_index(),
            account_names,
            connected: false, // Will be set true on Connected event
            emails_loaded,
            all_emails_loaded: emails_loaded < EMAIL_PAGE_SIZE,
            current_folder: default_folder,
            ai_polish_enabled: config.ai.is_enabled() && config.ai.enable_polish,
            ..Default::default()
        };

        // Initialize AI actor if enabled
        let ai_actor = if config.ai.is_enabled() {
            if let Some(ref api_key) = config.ai.api_key {
                let client = OpenRouterClient::new(api_key.clone(), config.ai.model.clone());
                Some(spawn_ai_actor(
                    client,
                    config.ai.summary_max_tokens,
                    config.ai.polish_max_tokens,
                ))
            } else {
                None
            }
        } else {
            None
        };

        let mut app = Self {
            config,
            cache,
            contacts,
            accounts,
            state,
            bindings,
            last_prefetch_uid: None,
            pending_prefetch: None,
            in_flight_fetches: HashSet::new(),
            prefetch_done: false,
            undo_stack: Vec::new(),
            pending_deletions: Vec::new(),
            last_search_input: None,
            ai_actor,
        };

        // Initialize other accounts info for status bar
        app.refresh_other_accounts_info();

        Ok(app)
    }

    /// Get current account_id
    pub(crate) fn account_id(&self) -> &str {
        &self.accounts.active().account_id
    }

    pub async fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        // Initial state - actor is connecting
        self.state.set_status("Connecting...");

        // Run event loop
        let result = self.event_loop(&mut terminal).await;

        // Flush any pending deletions before shutdown
        self.flush_pending_deletions().await;

        // Cleanup terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

        // Shutdown all IMAP actors
        self.accounts.shutdown().await;

        result
    }

    /// Flush all pending deletions immediately (for app shutdown)
    async fn flush_pending_deletions(&mut self) {
        use crate::mail::ImapCommand;

        let current_account = self.account_id().to_string();
        let pending: Vec<_> = self.pending_deletions.drain(..).collect();

        for pd in pending {
            // Only delete if we're still on the same account
            if pd.account_id == current_account {
                self.accounts
                    .send_command(ImapCommand::Delete { uid: pd.uid })
                    .await
                    .ok();
            }
        }
        self.undo_stack.clear();
    }

    /// Execute hybrid search: instant header search + async body FTS
    /// Called after debounce timeout to run the full body search
    pub(crate) async fn execute_search(&mut self) {
        use crate::mail::folder_cache_key;

        if self.state.search_query.is_empty() {
            self.state.update_search_cache_hybrid(HashSet::new());
            return;
        }

        let cache_key = folder_cache_key(self.account_id(), &self.state.current_folder);

        // Run async body FTS search
        let body_matches = self
            .cache
            .search_body_fts(&cache_key, &self.state.search_query)
            .await
            .unwrap_or_default();

        // Update search cache with both header and body matches
        self.state.update_search_cache_hybrid(body_matches);
    }

    /// Switch to the next account
    async fn switch_to_next_account(&mut self) {
        if self.accounts.count() <= 1 {
            self.state.set_status("Only one account configured");
            return;
        }

        self.accounts.next_account();
        self.on_account_switched().await;
    }

    /// Switch to the previous account
    async fn switch_to_prev_account(&mut self) {
        if self.accounts.count() <= 1 {
            self.state.set_status("Only one account configured");
            return;
        }

        self.accounts.prev_account();
        self.on_account_switched().await;
    }

    /// Called when the active account changes
    async fn on_account_switched(&mut self) {
        use crate::ui::app::View;

        // Update state for the new account
        let account = self.accounts.active();
        self.state.account_name = account.display_name().to_string();
        self.state.account_index = self.accounts.active_index();
        self.state.connected = account.connected;

        // Reload from cache FIRST to avoid visual flash
        // This loads new emails before we clear the selection state
        self.reload_from_cache().await;

        // Now clear state that wasn't overwritten by reload
        self.state.current_body = None;
        self.state.expanded_threads.clear();
        self.state.selected_thread = 0;
        self.state.selected_in_thread = 0;
        self.state.reader_scroll = 0;
        self.state.clear_search();
        self.in_flight_fetches.clear();
        self.last_prefetch_uid = None;
        self.pending_prefetch = None;

        // Update other accounts info for status bar
        self.refresh_other_accounts_info();

        // Update status
        self.state
            .set_status(format!("Switched to {}", self.state.account_name));

        // Go back to inbox view
        self.state.view = View::Inbox;
    }

    /// Extract sender addresses from cached emails and add to contacts
    pub(crate) async fn extract_contacts_from_emails(&mut self) {
        // Get the current emails and extract unique senders
        for email in &self.state.emails {
            // Add sender to contacts (will update existing or insert new)
            self.contacts
                .add_or_update(&email.from_addr, email.from_name.as_deref())
                .await
                .ok();
        }
    }

    /// Refresh the other_accounts info for status bar rendering
    pub(crate) fn refresh_other_accounts_info(&mut self) {
        use crate::ui::app::OtherAccountInfo;

        let active_index = self.accounts.active_index();
        self.state.other_accounts.clear();

        for (index, handle) in self.accounts.iter_enumerated() {
            if index == active_index {
                continue; // Skip the active account
            }

            self.state.other_accounts.push(OtherAccountInfo {
                name: handle.short_name(),
                has_new_mail: handle.has_new_mail,
                new_count: handle.unread_since_viewed,
                connected: handle.connected,
                has_error: handle.last_error.is_some(),
            });
        }
    }
}
