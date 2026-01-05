//! Application state types
//!
//! All state types live here to maintain clean dependency:
//! UI layer imports from app layer, not vice versa.

use std::collections::HashSet;

use aho_corasick::AhoCorasick;

use crate::command::{CommandHelp, CommandResult, PendingCommand};
use crate::constants::ERROR_TTL_SECS;
use crate::contacts::Contact;
use crate::input::KeybindingEntry;
use crate::mail::types::{Attachment, ComposeEmail, EmailBody, EmailHeader};
use crate::mail::{EmailThread, ThreadId};

/// Info about another account for the status bar indicators
#[derive(Debug, Clone, Default)]
pub struct OtherAccountInfo {
    /// Short display name for the account
    pub name: String,
    /// Whether there's new mail since last viewed
    pub has_new_mail: bool,
    /// Count of new messages (0 if none)
    pub new_count: usize,
    /// Whether the account is connected
    pub connected: bool,
    /// Whether the account has an error
    pub has_error: bool,
}

#[derive(Debug, Clone, Default)]
pub enum View {
    #[default]
    Inbox,
    Reader {
        uid: u32,
    },
    Composer {
        email: ComposeEmail,
        field: ComposerField,
    },
    /// Add account wizard
    AddAccount {
        step: AddAccountStep,
        data: AddAccountData,
    },
    /// Contacts view
    Contacts,
}

/// Steps in the add account wizard
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(dead_code)]
pub enum AddAccountStep {
    #[default]
    ChooseAuthMethod,
    EnterEmail,
    EnterPassword,
    OAuth2Flow,
    EnterImapServer,
    EnterSmtpServer,
    Confirm,
}

/// Data collected during add account wizard
#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct AddAccountData {
    pub auth_method: AddAccountAuth,
    pub email: String,
    pub password: String,
    pub imap_server: String,
    pub smtp_server: String,
    /// OAuth2 device code for display
    pub oauth2_user_code: Option<String>,
    /// OAuth2 verification URL
    pub oauth2_url: Option<String>,
    /// OAuth2 polling status message
    pub oauth2_status: Option<String>,
    /// Google OAuth2 client ID (user-provided or default)
    pub oauth2_client_id: Option<String>,
    /// OAuth2 device code (for polling - not shown to user)
    pub oauth2_device_code: Option<String>,
    /// OAuth2 refresh token (after successful auth)
    pub oauth2_refresh_token: Option<String>,
    /// Polling interval in seconds
    pub oauth2_poll_interval: Option<u64>,
}

/// Authentication method choice in wizard
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum AddAccountAuth {
    #[default]
    Password,
    OAuth2Gmail,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ComposerField {
    #[default]
    To,
    Cc,
    Subject,
    Body,
}

impl ComposerField {
    pub fn next(self) -> Self {
        match self {
            Self::To => Self::Cc,
            Self::Cc => Self::Subject,
            Self::Subject => Self::Body,
            Self::Body => Self::To,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::To => Self::Body,
            Self::Cc => Self::To,
            Self::Subject => Self::Cc,
            Self::Body => Self::Subject,
        }
    }
}

/// Modal overlay state - only one can be active at a time
#[derive(Debug, Clone, Default)]
pub enum ModalState {
    #[default]
    None,
    Search,
    Command {
        input: String,
        result: Option<CommandResult>,
        pending: Option<PendingCommand>,
    },
    FolderPicker,
    Help {
        keybindings: Vec<KeybindingEntry>,
        commands: Vec<CommandHelp>,
        scroll: usize,
    },
}

impl ModalState {
    pub fn is_search(&self) -> bool {
        matches!(self, Self::Search)
    }

    pub fn is_command(&self) -> bool {
        matches!(self, Self::Command { .. })
    }

    pub fn is_folder_picker(&self) -> bool {
        matches!(self, Self::FolderPicker)
    }

    pub fn is_help(&self) -> bool {
        matches!(self, Self::Help { .. })
    }

    pub fn is_active(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Get command input if in command mode
    pub fn command_input(&self) -> Option<&str> {
        match self {
            Self::Command { input, .. } => Some(input),
            _ => None,
        }
    }

    /// Get mutable command input if in command mode
    #[allow(dead_code)]
    pub fn command_input_mut(&mut self) -> Option<&mut String> {
        match self {
            Self::Command { input, .. } => Some(input),
            _ => None,
        }
    }

    /// Get command result if in command mode
    pub fn command_result(&self) -> Option<&CommandResult> {
        match self {
            Self::Command { result, .. } => result.as_ref(),
            _ => None,
        }
    }

    /// Get pending confirmation if in command mode
    pub fn pending_confirmation(&self) -> Option<&PendingCommand> {
        match self {
            Self::Command { pending, .. } => pending.as_ref(),
            _ => None,
        }
    }
}

/// View mode filter - show all emails or only starred
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ViewMode {
    #[default]
    All,
    Starred,
}

/// Type of match for search results - used to show [body] indicator
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    /// No match (shouldn't happen in visible results)
    None,
    /// Matched in subject/from headers only
    Header,
    /// Matched in email body only
    Body,
    /// Matched in both headers and body
    Both,
}

/// State for editing a contact's name
#[derive(Debug, Clone, Default)]
pub struct ContactEditState {
    pub contact_id: i64,
    pub name: String,
}

/// State for AI grammar/polish preview
#[derive(Debug, Clone)]
pub struct PolishPreview {
    /// Original text before polish
    pub original: String,
    /// Polished text from AI
    pub polished: String,
    /// Whether the AI request is still loading
    pub loading: bool,
}

/// Loading, error, and status message state
#[derive(Debug, Clone, Default)]
pub struct StatusState {
    pub loading: bool,
    pub error: Option<String>,
    pub error_time: Option<std::time::Instant>,
    pub message: String,
    /// Persists after error bar expires - shown as indicator in status bar
    pub has_unacknowledged_error: bool,
}

impl StatusState {
    pub fn set_error(&mut self, error: impl ToString) {
        self.error = Some(error.to_string());
        self.error_time = Some(std::time::Instant::now());
        self.has_unacknowledged_error = true;
    }

    pub fn clear_error(&mut self) {
        self.error = None;
        self.error_time = None;
    }

    /// Acknowledge the error indicator (clear the persistent flag)
    /// Call this on user input to dismiss the status bar indicator
    pub fn acknowledge_error(&mut self) {
        self.has_unacknowledged_error = false;
    }

    /// Clear error if it's been visible for more than the TTL
    /// Clear error if TTL expired. Returns true if error was cleared.
    pub fn clear_error_if_expired(&mut self) -> bool {
        if let Some(time) = self.error_time
            && time.elapsed().as_secs() >= ERROR_TTL_SECS
        {
            self.clear_error();
            true
        } else {
            false
        }
    }

    pub fn set_message(&mut self, msg: impl ToString) {
        self.message = msg.to_string();
    }
}

/// Pagination state for keyset pagination
/// Uses composite cursor (date, uid) for deterministic ordering when dates are identical
#[derive(Debug, Clone, Default)]
pub struct PaginationState {
    pub emails_loaded: usize,
    pub all_loaded: bool,
    /// Composite cursor: (date, uid) for deterministic pagination
    pub cursor: Option<(i64, u32)>,
}

/// Contacts view state
#[derive(Debug, Clone, Default)]
pub struct ContactsViewState {
    pub list: Vec<Contact>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub editing: Option<ContactEditState>,
}

/// Composer autocomplete state
#[derive(Debug, Clone, Default)]
pub struct AutocompleteState {
    pub suggestions: Vec<Contact>,
    pub selected: usize,
    pub visible: bool,
}

/// Folder state
#[derive(Debug, Clone, Default)]
pub struct FolderState {
    pub current: String,
    pub list: Vec<String>,
    pub picker_pending: bool,
    pub selected: usize,
}

/// Connection and account status
#[derive(Debug, Clone, Default)]
pub struct ConnectionState {
    pub connected: bool,
    pub last_sync: Option<i64>,
    pub account_name: String,
    pub account_index: usize,
    pub other_accounts: Vec<OtherAccountInfo>,
    pub account_names: Vec<String>,
}

impl FolderState {
    /// Returns display name, defaulting to "INBOX" if empty
    pub fn display_name(&self) -> &str {
        if self.current.is_empty() {
            "INBOX"
        } else {
            &self.current
        }
    }
}

impl ConnectionState {
    /// Returns display account name, defaulting to "Not connected" if empty
    pub fn display_account(&self) -> &str {
        if self.account_name.is_empty() {
            "Not connected"
        } else {
            &self.account_name
        }
    }
}

/// AI polish state
#[derive(Debug, Clone, Default)]
pub struct PolishState {
    pub preview: Option<PolishPreview>,
    pub enabled: bool,
}

/// Thread navigation state
#[derive(Debug, Clone, Default)]
pub struct ThreadState {
    pub threads: Vec<EmailThread>,
    pub expanded: HashSet<ThreadId>,
    pub selected: usize,
    pub selected_in_thread: usize, // For expanded threads (0 = thread header)
}

/// Search and filtering state
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    pub query: String,
    pub cached_visible_indices: Option<Vec<usize>>,
    pub cached_query: String,
    pub cached_view_mode: ViewMode,
    pub header_match_uids: HashSet<u32>,
    pub body_match_uids: HashSet<u32>,
    /// Cached Aho-Corasick automaton for search (rebuilt when query changes).
    /// Uses RefCell for interior mutability in compute_visible_threads.
    cached_automaton: std::cell::RefCell<Option<(String, AhoCorasick)>>,
}

/// Reader view state
#[derive(Debug, Clone, Default)]
pub struct ReaderState {
    pub body: Option<EmailBody>,
    /// Cached sanitized body text (invalidated when body changes).
    /// Uses RefCell for interior mutability so rendering can populate cache.
    pub cached_sanitized: std::cell::RefCell<Option<String>>,
    pub scroll: usize,
    pub show_summary: bool,
    pub cached_summary: Option<(u32, String)>,
    pub cached_thread_summary: Option<(ThreadId, String)>,
    pub summary_loading: bool,
    /// Whether email headers in preview are expanded (show all lines instead of 3 per field)
    pub headers_expanded: bool,
    /// List of attachments for the current email
    pub attachments: Vec<Attachment>,
    /// Currently selected attachment index (for keyboard navigation)
    pub attachment_selected: usize,
    /// Whether attachment list is focused
    pub show_attachments: bool,
    /// Pending attachment save (index, save path)
    pub pending_attachment_save: Option<(usize, std::path::PathBuf)>,
}

impl ReaderState {
    /// Set body and invalidate sanitized cache
    pub fn set_body(&mut self, body: Option<EmailBody>) {
        self.body = body;
        *self.cached_sanitized.borrow_mut() = None;
    }

    /// Get sanitized body text, computing and caching if needed.
    /// Uses interior mutability to cache on first access.
    pub fn sanitized_body(&self, sanitize_fn: fn(&str) -> String) -> String {
        {
            let cache = self.cached_sanitized.borrow();
            if let Some(ref s) = *cache {
                return s.clone();
            }
        }
        // Cache miss - compute and store
        let sanitized = if let Some(ref body) = self.body {
            sanitize_fn(&body.display_text())
        } else {
            String::new()
        };
        *self.cached_sanitized.borrow_mut() = Some(sanitized.clone());
        sanitized
    }

    /// Get maximum scroll value based on current content
    pub fn max_scroll(&self) -> usize {
        if let Some(ref body) = self.body {
            body.display_text().lines().count().saturating_sub(1)
        } else {
            0
        }
    }

    /// Scroll down by one line (bounded)
    pub fn scroll_down(&mut self) {
        let max = self.max_scroll();
        if self.scroll < max {
            self.scroll += 1;
        }
    }

    /// Scroll up by one line
    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }

    /// Scroll by delta (bounded)
    pub fn scroll_by(&mut self, delta: i32) {
        let max = self.max_scroll();
        let new_scroll = (self.scroll as i32 + delta).clamp(0, max as i32);
        self.scroll = new_scroll as usize;
    }

    /// Reset scroll when changing emails
    pub fn reset_scroll(&mut self) {
        self.scroll = 0;
    }

    /// Move attachment selection down
    pub fn attachment_down(&mut self) {
        if !self.attachments.is_empty() && self.attachment_selected < self.attachments.len() - 1 {
            self.attachment_selected += 1;
        }
    }

    /// Move attachment selection up
    pub fn attachment_up(&mut self) {
        if self.attachment_selected > 0 {
            self.attachment_selected -= 1;
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppState {
    pub view: View,
    pub emails: Vec<EmailHeader>,
    pub selected: usize,
    pub scroll_offset: usize,

    // Status state (loading, error, status message)
    pub status: StatusState,

    // Thread state
    pub thread: ThreadState,

    // Search state
    pub search: SearchState,

    // Reader state
    pub reader: ReaderState,

    // Stats
    pub total_count: usize,
    pub unread_count: usize,

    // UI settings
    pub split_ratio: u16,
    /// Show sent emails in inbox threads (conversation view)
    pub conversation_mode: bool,

    // Modal overlay state (search, command, folder picker)
    pub modal: ModalState,

    // Folder state
    pub folder: FolderState,

    // Connection and account status
    pub connection: ConnectionState,

    // View mode (all emails or starred only)
    pub view_mode: ViewMode,

    // Pagination state (keyset pagination using date cursor)
    pub pagination: PaginationState,

    // Contacts view state
    pub contacts: ContactsViewState,

    // Composer autocomplete state
    pub autocomplete: AutocompleteState,

    // AI polish state
    pub polish: PolishState,
}

impl AppState {
    #[allow(dead_code)]
    pub fn selected_email(&self) -> Option<&EmailHeader> {
        self.emails.get(self.selected)
    }

    /// Get the currently selected thread (respects search filter)
    pub fn current_thread(&self) -> Option<&EmailThread> {
        let visible = self.visible_threads();
        visible.get(self.thread.selected).copied()
    }

    /// Get the currently selected email (considering expanded threads)
    pub fn current_email_from_thread(&self) -> Option<&EmailHeader> {
        let thread = self.current_thread()?;
        if self.is_thread_expanded(&thread.id) && self.thread.selected_in_thread > 0 {
            // In expanded view, selected_in_thread 1 = first email, 2 = second, etc.
            thread.email_at(&self.emails, self.thread.selected_in_thread - 1)
        } else {
            // Collapsed view or header selected - return latest email
            Some(thread.latest(&self.emails))
        }
    }

    /// Check if a thread is expanded
    pub fn is_thread_expanded(&self, thread_id: &ThreadId) -> bool {
        self.thread.expanded.contains(thread_id)
    }

    /// Toggle thread expansion
    pub fn toggle_thread_expansion(&mut self) {
        if let Some(thread) = self.current_thread() {
            let id = thread.id.clone();
            let count = thread.total_count;
            if self.thread.expanded.contains(&id) {
                self.thread.expanded.remove(&id);
                self.thread.selected_in_thread = 0;
            } else if count > 1 {
                // Only expand threads with multiple emails
                self.thread.expanded.insert(id);
                self.thread.selected_in_thread = 0;
            }
        }
    }

    /// Move selection down (thread-aware, respects search filter)
    pub fn move_down(&mut self) {
        let visible = self.visible_threads();
        if visible.is_empty() {
            return;
        }

        if let Some(thread) = visible.get(self.thread.selected)
            && self.is_thread_expanded(&thread.id)
        {
            // In expanded thread
            let max_in_thread = thread.len();
            if self.thread.selected_in_thread < max_in_thread {
                self.thread.selected_in_thread += 1;
                return;
            }
        }

        // Move to next thread
        if self.thread.selected < visible.len() - 1 {
            self.thread.selected += 1;
            self.thread.selected_in_thread = 0;
        }
    }

    /// Move selection up (thread-aware, respects search filter)
    pub fn move_up(&mut self) {
        let visible_len = self.visible_thread_count();
        if visible_len == 0 {
            return;
        }

        if self.thread.selected_in_thread > 0 {
            self.thread.selected_in_thread -= 1;
            return;
        }

        if self.thread.selected > 0 {
            self.thread.selected -= 1;
            // If moving into an expanded thread, go to its last item
            if let Some(thread) = self.current_thread()
                && self.is_thread_expanded(&thread.id)
            {
                self.thread.selected_in_thread = thread.len();
            }
        }
    }

    /// Collapse current thread and move up
    pub fn collapse_or_move_left(&mut self) {
        if let Some(thread) = self.current_thread()
            && self.is_thread_expanded(&thread.id)
        {
            let id = thread.id.clone();
            self.thread.expanded.remove(&id);
            self.thread.selected_in_thread = 0;
        }
    }

    /// Expand current thread
    pub fn expand_thread(&mut self) {
        if let Some(thread) = self.current_thread()
            && thread.total_count > 1
            && !self.is_thread_expanded(&thread.id)
        {
            let id = thread.id.clone();
            self.thread.expanded.insert(id);
        }
    }

    /// Clamp selection to visible threads (call after search/filter changes)
    /// This prevents selection from pointing to a non-visible thread
    pub fn clamp_selection_to_visible(&mut self) {
        let visible_count = self.visible_thread_count();
        if visible_count == 0 {
            self.thread.selected = 0;
            self.thread.selected_in_thread = 0;
            return;
        }
        if self.thread.selected >= visible_count {
            self.thread.selected = visible_count.saturating_sub(1);
            self.thread.selected_in_thread = 0;
        }
    }

    // Delegate methods to StatusState
    pub fn set_error(&mut self, error: impl ToString) {
        self.status.set_error(error);
    }

    pub fn clear_error(&mut self) {
        self.status.clear_error();
    }

    pub fn clear_error_if_expired(&mut self) -> bool {
        self.status.clear_error_if_expired()
    }

    pub fn acknowledge_error(&mut self) {
        self.status.acknowledge_error();
    }

    pub fn has_unacknowledged_error(&self) -> bool {
        self.status.has_unacknowledged_error
    }

    pub fn set_status(&mut self, msg: impl ToString) {
        self.status.set_message(msg);
    }

    /// Get count of visible threads (O(1) from cache when possible).
    /// Prefer this over `visible_threads().len()` to avoid allocations.
    pub fn visible_thread_count(&self) -> usize {
        if let Some(ref indices) = self.search.cached_visible_indices
            && self.search.query == self.search.cached_query
            && self.view_mode == self.search.cached_view_mode
        {
            indices.len()
        } else {
            // Cache miss - fall back to compute
            self.compute_visible_threads().len()
        }
    }

    /// Get cached visible thread indices as a slice.
    /// Returns None if cache needs rebuilding (caller should use visible_threads() instead).
    /// This avoids allocation when the caller can iterate indices directly.
    #[allow(dead_code)] // Reserved for future use
    pub fn visible_thread_indices(&self) -> Option<&[usize]> {
        if let Some(ref indices) = self.search.cached_visible_indices
            && self.search.query == self.search.cached_query
            && self.view_mode == self.search.cached_view_mode
        {
            Some(indices.as_slice())
        } else {
            None
        }
    }

    /// Get threads filtered by search query and view mode.
    /// Uses cached indices when possible (1000x faster for repeated calls).
    pub fn visible_threads(&self) -> Vec<&EmailThread> {
        // Check if we have valid cached results
        if let Some(ref indices) = self.search.cached_visible_indices
            && self.search.query == self.search.cached_query
            && self.view_mode == self.search.cached_view_mode
        {
            return indices
                .iter()
                .filter_map(|&i| self.thread.threads.get(i))
                .collect();
        }

        // Cache miss - compute and return (caller should call invalidate_search_cache on changes)
        self.compute_visible_threads()
    }

    /// Compute visible threads without caching (internal helper)
    /// Uses aho-corasick for fast O(n) pattern matching
    fn compute_visible_threads(&self) -> Vec<&EmailThread> {
        let filtered: Vec<&EmailThread> = if self.search.query.is_empty() {
            self.thread.threads.iter().collect()
        } else {
            // Get or build aho-corasick automaton (cached for repeated calls with same query)
            let query_lower = self.search.query.to_lowercase();

            // Check if we have a cached automaton for this query
            let needs_rebuild = {
                let cache = self.search.cached_automaton.borrow();
                cache
                    .as_ref()
                    .map(|(q, _)| q != &query_lower)
                    .unwrap_or(true)
            };

            if needs_rebuild {
                // Build new automaton and cache it
                let ac = match AhoCorasick::new([&query_lower]) {
                    Ok(ac) => ac,
                    Err(_) => return self.thread.threads.iter().collect(),
                };
                *self.search.cached_automaton.borrow_mut() = Some((query_lower.clone(), ac));
            }

            let cache = self.search.cached_automaton.borrow();
            let ac = &cache.as_ref().unwrap().1;

            self.thread
                .threads
                .iter()
                .filter(|thread| {
                    thread.emails(&self.emails).any(|email| {
                        // aho-corasick is_match is O(n) in text length
                        ac.is_match(&email.subject.to_lowercase())
                            || ac.is_match(&email.from_addr.to_lowercase())
                            || email
                                .from_name
                                .as_ref()
                                .map(|n| ac.is_match(&n.to_lowercase()))
                                .unwrap_or(false)
                    })
                })
                .collect()
        };

        // Apply view mode filter
        match self.view_mode {
            ViewMode::All => filtered,
            ViewMode::Starred => filtered
                .into_iter()
                .filter(|thread| thread.emails(&self.emails).any(|e| e.is_flagged()))
                .collect(),
        }
    }

    /// Invalidate the search cache (call when threads change)
    pub fn invalidate_search_cache(&mut self) {
        self.search.cached_visible_indices = None;
    }

    /// Toggle between all emails and starred-only view
    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::All => ViewMode::Starred,
            ViewMode::Starred => ViewMode::All,
        };
        // Update search cache with new view mode, preserving existing body matches
        let existing_body_matches = self.search.body_match_uids.clone();
        self.update_search_cache_hybrid(existing_body_matches);
        // Reset selection when switching modes
        self.thread.selected = 0;
        self.thread.selected_in_thread = 0;
    }

    /// Check if currently in starred view
    pub fn is_starred_view(&self) -> bool {
        matches!(self.view_mode, ViewMode::Starred)
    }

    /// Clear search and reset selection
    pub fn clear_search(&mut self) {
        self.search.query.clear();
        // Invalidate cache since search changed
        self.invalidate_search_cache();
        // Clear match tracking
        self.search.header_match_uids.clear();
        self.search.body_match_uids.clear();
        if self.modal.is_search() {
            self.modal = ModalState::None;
        }
        self.thread.selected = 0;
        self.thread.selected_in_thread = 0;
        self.scroll_offset = 0;
    }

    /// Get the match type for a given email UID
    /// Used to show [body] indicator in search results
    pub fn get_match_type(&self, uid: u32) -> MatchType {
        let in_headers = self.search.header_match_uids.contains(&uid);
        let in_body = self.search.body_match_uids.contains(&uid);
        match (in_headers, in_body) {
            (true, true) => MatchType::Both,
            (true, false) => MatchType::Header,
            (false, true) => MatchType::Body,
            (false, false) => MatchType::None,
        }
    }

    /// Compute header matches using aho-corasick (instant, in-memory)
    /// Returns UIDs of emails matching the current search query in headers
    pub fn compute_header_matches(&self) -> HashSet<u32> {
        if self.search.query.is_empty() {
            return HashSet::new();
        }

        let query_lower = self.search.query.to_lowercase();
        let ac = match AhoCorasick::new([&query_lower]) {
            Ok(ac) => ac,
            Err(_) => return HashSet::new(),
        };

        self.emails
            .iter()
            .filter(|email| {
                ac.is_match(&email.subject.to_lowercase())
                    || ac.is_match(&email.from_addr.to_lowercase())
                    || email
                        .from_name
                        .as_ref()
                        .map(|n| ac.is_match(&n.to_lowercase()))
                        .unwrap_or(false)
            })
            .map(|e| e.uid)
            .collect()
    }

    /// Update search cache with hybrid results (header + body matches)
    /// Call with body_matches from FTS (can be empty if body search not ready yet)
    pub fn update_search_cache_hybrid(&mut self, body_matches: HashSet<u32>) {
        // Compute header matches
        let header_matches = self.compute_header_matches();

        // Store match tracking for [body] indicator
        self.search.header_match_uids = header_matches.clone();
        self.search.body_match_uids = body_matches.clone();

        if self.search.query.is_empty() && self.view_mode == ViewMode::All {
            self.search.cached_visible_indices = Some((0..self.thread.threads.len()).collect());
            self.search.cached_query = self.search.query.clone();
            self.search.cached_view_mode = self.view_mode;
            // Clamp selection in case threads changed
            self.clamp_selection_to_visible();
            return;
        }

        // Merge: thread visible if ANY email matches headers OR body
        let indices: Vec<usize> = self
            .thread
            .threads
            .iter()
            .enumerate()
            .filter(|(_, thread)| {
                let matches_search = self.search.query.is_empty()
                    || thread.email_indices.iter().any(|&idx| {
                        let uid = self.emails[idx].uid;
                        header_matches.contains(&uid) || body_matches.contains(&uid)
                    });

                let matches_view = match self.view_mode {
                    ViewMode::All => true,
                    ViewMode::Starred => thread.emails(&self.emails).any(|e| e.is_flagged()),
                };

                matches_search && matches_view
            })
            .map(|(i, _)| i)
            .collect();

        self.search.cached_visible_indices = Some(indices);
        self.search.cached_query = self.search.query.clone();
        self.search.cached_view_mode = self.view_mode;

        // Clamp selection to visible threads after filter change
        self.clamp_selection_to_visible();
    }

    /// Check if we need to load more emails (user is near the bottom of the list)
    pub fn needs_more_emails(&self) -> bool {
        if self.pagination.all_loaded || self.status.loading {
            return false;
        }
        // Load more when within 20 threads of the end
        let visible = self.visible_threads();
        let threshold = 20;
        self.thread.selected + threshold >= visible.len()
    }

    /// Get UIDs of nearby emails for prefetching (current + adjacent)
    /// Returns UIDs in priority order: current first, then nearby
    pub fn nearby_email_uids(&self, radius: usize) -> Vec<u32> {
        let mut uids = Vec::new();
        let visible = self.visible_threads();

        if visible.is_empty() {
            return uids;
        }

        // Get current email UID (highest priority)
        if let Some(current) = self.current_email_from_thread() {
            uids.push(current.uid);
        }

        if radius == 0 {
            return uids;
        }

        // Helper to add UID if not already present
        let mut add_uid = |uid: u32| {
            if !uids.contains(&uid) {
                uids.push(uid);
            }
        };

        // Get the current thread
        let current_thread = match visible.get(self.thread.selected) {
            Some(t) => *t,
            None => return uids,
        };

        // If thread is expanded, add adjacent emails within the thread
        if self.is_thread_expanded(&current_thread.id) {
            let in_thread = self.thread.selected_in_thread;
            for offset in 1..=radius {
                // Email below in thread
                if in_thread > 0
                    && in_thread - 1 + offset < current_thread.len()
                    && let Some(email) =
                        current_thread.email_at(&self.emails, in_thread - 1 + offset)
                {
                    add_uid(email.uid);
                }
                // Email above in thread
                if in_thread > offset {
                    if let Some(email) =
                        current_thread.email_at(&self.emails, in_thread - 1 - offset)
                    {
                        add_uid(email.uid);
                    }
                } else if in_thread == 0 && offset <= current_thread.len() {
                    // At thread header, get emails from thread
                    if let Some(email) = current_thread.email_at(&self.emails, offset - 1) {
                        add_uid(email.uid);
                    }
                }
            }
        }

        // Add latest emails from adjacent threads
        for offset in 1..=radius {
            // Thread below
            if self.thread.selected + offset < visible.len() {
                add_uid(
                    visible[self.thread.selected + offset]
                        .latest(&self.emails)
                        .uid,
                );
            }
            // Thread above
            if self.thread.selected >= offset {
                add_uid(
                    visible[self.thread.selected - offset]
                        .latest(&self.emails)
                        .uid,
                );
            }
        }

        uids
    }
}
