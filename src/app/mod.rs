//! Application core - manages state, accounts, and coordination

mod actions;
mod event_loop;
mod handlers;
pub mod render_thread;
pub mod state;
pub mod undo;

use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use render_thread::RenderThread;

use crate::account::AccountManager;
use crate::ai::{AiActorHandle, OpenRouterClient, spawn_ai_actor};
use crate::cache::Cache;
use crate::config::Config;
use crate::constants::{EMAIL_PAGE_SIZE, PREFETCH_DEBOUNCE_MS};
use crate::contacts::ContactsDb;
use crate::credentials::CredentialStore;
use crate::input::KeyBindings;
use crate::mail::{folder_cache_key, group_into_threads};
use state::{
    AppState, ConnectionState, FolderState, PaginationState, PolishState, StatusState, ThreadState,
};

use self::undo::{PendingDeletion, UndoEntry};

/// State for email body prefetching and debouncing
#[derive(Debug, Default)]
pub struct PrefetchState {
    /// Track last prefetch UID to avoid redundant requests
    pub last_uid: Option<u32>,
    /// Pending prefetch: (uids, when_requested) - for debouncing rapid navigation
    /// First UID is the current selection; remaining are nearby emails
    pub pending: Option<(Vec<u32>, Instant)>,
    /// UIDs currently being fetched (prevents duplicate requests)
    pub in_flight: HashSet<u32>,
    /// Whether we've done initial folder prefetch (after first INBOX sync)
    pub folder_done: bool,
    /// Whether folder prefetch is pending (waiting for folder list)
    pub folder_pending: bool,
}

impl PrefetchState {
    /// Clear all prefetch state (used on folder/account switch)
    pub fn clear(&mut self) {
        self.last_uid = None;
        self.pending = None;
        self.in_flight.clear();
    }
}

pub struct App {
    pub(crate) config: Config,
    pub(crate) cache: Arc<Cache>,
    pub(crate) contacts: ContactsDb,
    pub(crate) accounts: AccountManager,
    pub(crate) state: AppState,
    pub(crate) bindings: KeyBindings,
    /// Email body prefetching state
    pub(crate) prefetch: PrefetchState,
    /// Stack of undoable actions (most recent first)
    pub(crate) undo_stack: Vec<UndoEntry>,
    /// Pending deletions waiting to be executed (delayed by 10 seconds)
    pub(crate) pending_deletions: Vec<PendingDeletion>,
    /// Tracks when the last search input was received (for debouncing body FTS)
    pub(crate) last_search_input: Option<Instant>,
    /// AI actor handle for summarization and polish (None if AI features disabled)
    pub(crate) ai_actor: Option<AiActorHandle>,
    /// Dirty flag: when true, UI needs re-render. Skips renders when nothing changed.
    pub(crate) dirty: bool,
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
            thread: ThreadState {
                threads,
                ..Default::default()
            },
            total_count,
            unread_count,
            status: StatusState {
                loading: true, // Actor is connecting
                ..Default::default()
            },
            split_ratio: config.ui.split_ratio.clamp(30, 70),
            conversation_mode: config.ui.conversation_mode,
            connection: ConnectionState {
                account_name,
                account_index: accounts.active_index(),
                account_names,
                connected: false, // Will be set true on Connected event
                ..Default::default()
            },
            pagination: PaginationState {
                emails_loaded,
                all_loaded: emails_loaded < EMAIL_PAGE_SIZE,
                ..Default::default()
            },
            folder: FolderState {
                current: default_folder,
                ..Default::default()
            },
            polish: PolishState {
                enabled: config.ai.is_enabled() && config.ai.enable_polish,
                ..Default::default()
            },
            ..Default::default()
        };

        // Initialize AI actor if enabled
        let ai_actor = if config.ai.is_enabled() {
            if let Some(api_key) = config.ai.get_api_key() {
                let client = OpenRouterClient::new(api_key, config.ai.model.clone());
                Some(spawn_ai_actor(
                    client,
                    config.ai.summary_max_tokens,
                    config.ai.thread_summary_max_tokens,
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
            prefetch: PrefetchState::default(),
            undo_stack: Vec::new(),
            pending_deletions: Vec::new(),
            last_search_input: None,
            ai_actor,
            dirty: true, // Start dirty for initial render
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
        // Spawn background render thread (owns terminal setup/teardown)
        let render_thread = RenderThread::spawn()?;

        // Initial state - actor is connecting
        self.state.set_status("Connecting...");

        // Run event loop
        let result = self.event_loop(&render_thread).await;

        // Flush any pending deletions before shutdown
        self.flush_pending_deletions().await;

        // Shutdown render thread (handles terminal cleanup)
        render_thread.shutdown();

        // Shutdown all IMAP actors
        self.accounts.shutdown().await;

        result
    }

    /// Flush all pending deletions immediately (for app shutdown)
    async fn flush_pending_deletions(&mut self) {
        use crate::mail::ImapCommand;

        let pending: Vec<_> = self.pending_deletions.drain(..).collect();

        for pd in pending {
            // Route deletion to correct account (even if we switched accounts)
            if let Some(account_idx) = self.accounts.index_of(&pd.account_id) {
                self.accounts
                    .send_command_to(
                        account_idx,
                        ImapCommand::Delete {
                            uid: pd.uid,
                            folder: pd.folder,
                        },
                    )
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

        if self.state.search.query.is_empty() {
            self.state.update_search_cache_hybrid(HashSet::new());
            return;
        }

        let cache_key = folder_cache_key(self.account_id(), &self.state.folder.current);

        // Run async body FTS search
        let body_matches = self
            .cache
            .search_body_fts(&cache_key, &self.state.search.query)
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
        use crate::app::state::View;

        // Update state for the new account
        let account = self.accounts.active();
        self.state.connection.account_name = account.display_name().to_string();
        self.state.connection.account_index = self.accounts.active_index();
        self.state.connection.connected = account.connected;

        // Sync folder list from account handle to state
        if !account.folder_list.is_empty() {
            self.state.folder.list = account.folder_list.clone();
        } else {
            self.state.folder.list.clear();
        }
        // Reset folder selection state
        self.state.folder.selected = 0;

        // Reload from cache FIRST to avoid visual flash
        // This loads new emails before we clear the selection state
        self.reload_from_cache().await;

        // Now clear state that wasn't overwritten by reload
        self.state.reader.set_body(None);
        self.state.thread.expanded.clear();
        self.state.thread.selected = 0;
        self.state.thread.selected_in_thread = 0;
        self.state.reader.scroll = 0;
        self.state.clear_search();
        self.prefetch.clear();

        // Update other accounts info for status bar
        self.refresh_other_accounts_info();

        // Update status
        self.state.set_status(format!(
            "Switched to {}",
            self.state.connection.account_name
        ));

        // Go back to inbox view
        self.state.view = View::Inbox;

        // Schedule prefetch for the selected email in the new account
        self.schedule_prefetch().await;
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
        use crate::app::state::OtherAccountInfo;

        let active_index = self.accounts.active_index();
        self.state.connection.other_accounts.clear();

        for (index, handle) in self.accounts.iter_enumerated() {
            if index == active_index {
                continue; // Skip the active account
            }

            self.state.connection.other_accounts.push(OtherAccountInfo {
                name: handle.short_name(),
                has_new_mail: handle.has_new_mail,
                new_count: handle.unread_since_viewed,
                connected: handle.connected,
                has_error: handle.last_error.is_some(),
            });
        }
    }
}
