use std::collections::HashSet;

use aho_corasick::AhoCorasick;
use ratatui::Frame;

use crate::command::{CommandResult, PendingCommand};
use crate::constants::ERROR_TTL_SECS;
use crate::contacts::Contact;
use crate::mail::types::{ComposeEmail, EmailBody, EmailHeader};
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

use super::add_account::render_add_account;
use super::composer::render_composer;
use super::contacts::render_contacts;
use super::inbox::render_inbox;
use super::reader::render_reader;

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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ModalState {
    #[default]
    None,
    Search,
    Command,
    FolderPicker,
}

impl ModalState {
    pub fn is_search(&self) -> bool {
        matches!(self, Self::Search)
    }

    pub fn is_command(&self) -> bool {
        matches!(self, Self::Command)
    }

    pub fn is_folder_picker(&self) -> bool {
        matches!(self, Self::FolderPicker)
    }

    pub fn is_active(&self) -> bool {
        !matches!(self, Self::None)
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

#[derive(Debug, Clone, Default)]
pub struct AppState {
    pub view: View,
    pub emails: Vec<EmailHeader>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub loading: bool,
    pub error: Option<String>,
    pub error_time: Option<std::time::Instant>, // When error was set (for TTL)
    pub status: String,

    // Thread state
    pub threads: Vec<EmailThread>,
    pub expanded_threads: HashSet<ThreadId>,
    pub selected_thread: usize,
    pub selected_in_thread: usize, // For expanded threads (0 = thread header)

    // Search result caching (avoids O(n*m) per-frame filtering)
    pub cached_visible_indices: Option<Vec<usize>>,
    pub cached_search_query: String,
    pub cached_view_mode: ViewMode,

    // Reader state
    pub current_body: Option<EmailBody>,
    pub reader_scroll: usize,

    // Stats
    pub total_count: usize,
    pub unread_count: usize,

    // UI settings
    pub split_ratio: u16,

    // Modal overlay state (search, command, folder picker)
    pub modal: ModalState,

    // Search state
    pub search_query: String,

    // Command mode state
    pub command_input: String,
    pub command_result: Option<CommandResult>,
    pub pending_confirmation: Option<PendingCommand>,

    // Folder state
    pub current_folder: String,
    pub folders: Vec<String>,
    pub folder_picker_pending: bool, // Auto-open picker when folders load
    pub folder_selected: usize,

    // Connection status
    pub connected: bool,
    pub last_sync: Option<i64>, // Unix timestamp of last successful sync
    pub account_name: String,   // Email account identifier

    // Multi-account state
    pub account_index: usize, // Index of currently active account
    pub other_accounts: Vec<OtherAccountInfo>, // Info about other accounts for status bar
    pub account_names: Vec<String>, // Display names for all accounts (for composer)

    // View mode (all emails or starred only)
    pub view_mode: ViewMode,

    // Pagination state (keyset pagination using date cursor)
    pub emails_loaded: usize,           // Number of emails currently loaded
    pub all_emails_loaded: bool,        // Whether all emails have been loaded from cache
    pub pagination_cursor: Option<i64>, // Date of oldest loaded email (for keyset pagination)

    // Contacts view state
    pub contacts_list: Vec<Contact>,
    pub contacts_selected: usize,
    pub contacts_scroll_offset: usize,
    pub contacts_editing: Option<ContactEditState>,

    // Composer autocomplete state
    pub autocomplete_suggestions: Vec<Contact>,
    pub autocomplete_selected: usize,
    pub autocomplete_visible: bool,

    // Search match tracking (for [body] indicator and highlighting)
    pub header_match_uids: HashSet<u32>,
    pub body_match_uids: HashSet<u32>,

    // AI summarization state
    /// Whether reader is showing AI summary vs full email
    pub reader_show_summary: bool,
    /// Cached AI summary for current email (uid, summary)
    pub cached_summary: Option<(u32, String)>,
    /// Cached AI summary for current thread (thread_id, summary)
    pub cached_thread_summary: Option<(ThreadId, String)>,
    /// Whether AI summary is loading
    pub summary_loading: bool,

    // AI polish state
    /// Polish preview for composer (None = not active)
    pub polish_preview: Option<PolishPreview>,
    /// Whether AI polish feature is enabled (for UI hints)
    pub ai_polish_enabled: bool,
}

impl AppState {
    #[allow(dead_code)]
    pub fn selected_email(&self) -> Option<&EmailHeader> {
        self.emails.get(self.selected)
    }

    /// Get the currently selected thread (respects search filter)
    pub fn current_thread(&self) -> Option<&EmailThread> {
        let visible = self.visible_threads();
        visible.get(self.selected_thread).copied()
    }

    /// Get the currently selected email (considering expanded threads)
    pub fn current_email_from_thread(&self) -> Option<&EmailHeader> {
        let thread = self.current_thread()?;
        if self.is_thread_expanded(&thread.id) && self.selected_in_thread > 0 {
            // In expanded view, selected_in_thread 1 = first email, 2 = second, etc.
            thread.email_at(&self.emails, self.selected_in_thread - 1)
        } else {
            // Collapsed view or header selected - return latest email
            Some(thread.latest(&self.emails))
        }
    }

    /// Check if a thread is expanded
    pub fn is_thread_expanded(&self, thread_id: &ThreadId) -> bool {
        self.expanded_threads.contains(thread_id)
    }

    /// Toggle thread expansion
    pub fn toggle_thread_expansion(&mut self) {
        if let Some(thread) = self.current_thread() {
            let id = thread.id.clone();
            let count = thread.total_count;
            if self.expanded_threads.contains(&id) {
                self.expanded_threads.remove(&id);
                self.selected_in_thread = 0;
            } else if count > 1 {
                // Only expand threads with multiple emails
                self.expanded_threads.insert(id);
                self.selected_in_thread = 0;
            }
        }
    }

    /// Move selection down (thread-aware, respects search filter)
    pub fn move_down(&mut self) {
        let visible = self.visible_threads();
        if visible.is_empty() {
            return;
        }

        if let Some(thread) = visible.get(self.selected_thread) {
            if self.is_thread_expanded(&thread.id) {
                // In expanded thread
                let max_in_thread = thread.len();
                if self.selected_in_thread < max_in_thread {
                    self.selected_in_thread += 1;
                    return;
                }
            }
        }

        // Move to next thread
        if self.selected_thread < visible.len() - 1 {
            self.selected_thread += 1;
            self.selected_in_thread = 0;
        }
    }

    /// Move selection up (thread-aware, respects search filter)
    pub fn move_up(&mut self) {
        let visible_len = self.visible_threads().len();
        if visible_len == 0 {
            return;
        }

        if self.selected_in_thread > 0 {
            self.selected_in_thread -= 1;
            return;
        }

        if self.selected_thread > 0 {
            self.selected_thread -= 1;
            // If moving into an expanded thread, go to its last item
            if let Some(thread) = self.current_thread() {
                if self.is_thread_expanded(&thread.id) {
                    self.selected_in_thread = thread.len();
                }
            }
        }
    }

    /// Collapse current thread and move up
    pub fn collapse_or_move_left(&mut self) {
        if let Some(thread) = self.current_thread() {
            if self.is_thread_expanded(&thread.id) {
                let id = thread.id.clone();
                self.expanded_threads.remove(&id);
                self.selected_in_thread = 0;
            }
        }
    }

    /// Expand current thread
    pub fn expand_thread(&mut self) {
        if let Some(thread) = self.current_thread() {
            if thread.total_count > 1 && !self.is_thread_expanded(&thread.id) {
                let id = thread.id.clone();
                self.expanded_threads.insert(id);
            }
        }
    }

    pub fn set_error(&mut self, error: impl ToString) {
        self.error = Some(error.to_string());
        self.error_time = Some(std::time::Instant::now());
    }

    pub fn clear_error(&mut self) {
        self.error = None;
        self.error_time = None;
    }

    /// Clear error if it's been visible for more than the TTL
    pub fn clear_error_if_expired(&mut self) {
        if let Some(time) = self.error_time {
            if time.elapsed().as_secs() >= ERROR_TTL_SECS {
                self.clear_error();
            }
        }
    }

    pub fn set_status(&mut self, status: impl ToString) {
        self.status = status.to_string();
    }

    /// Get threads filtered by search query and view mode.
    /// Uses cached indices when possible (1000x faster for repeated calls).
    pub fn visible_threads(&self) -> Vec<&EmailThread> {
        // Check if we have valid cached results
        if let Some(ref indices) = self.cached_visible_indices {
            if self.search_query == self.cached_search_query
                && self.view_mode == self.cached_view_mode
            {
                return indices
                    .iter()
                    .filter_map(|&i| self.threads.get(i))
                    .collect();
            }
        }

        // Cache miss - compute and return (caller should call invalidate_search_cache on changes)
        self.compute_visible_threads()
    }

    /// Compute visible threads without caching (internal helper)
    /// Uses aho-corasick for fast O(n) pattern matching
    fn compute_visible_threads(&self) -> Vec<&EmailThread> {
        let filtered: Vec<&EmailThread> = if self.search_query.is_empty() {
            self.threads.iter().collect()
        } else {
            // Build aho-corasick automaton for fast matching
            let query_lower = self.search_query.to_lowercase();
            let ac = match AhoCorasick::new([&query_lower]) {
                Ok(ac) => ac,
                Err(_) => return self.threads.iter().collect(), // Fallback: show all threads on invalid pattern
            };

            self.threads
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
        self.cached_visible_indices = None;
    }

    /// Toggle between all emails and starred-only view
    pub fn toggle_view_mode(&mut self) {
        self.view_mode = match self.view_mode {
            ViewMode::All => ViewMode::Starred,
            ViewMode::Starred => ViewMode::All,
        };
        // Update search cache with new view mode, preserving existing body matches
        let existing_body_matches = self.body_match_uids.clone();
        self.update_search_cache_hybrid(existing_body_matches);
        // Reset selection when switching modes
        self.selected_thread = 0;
        self.selected_in_thread = 0;
    }

    /// Check if currently in starred view
    pub fn is_starred_view(&self) -> bool {
        matches!(self.view_mode, ViewMode::Starred)
    }

    /// Clear search and reset selection
    pub fn clear_search(&mut self) {
        self.search_query.clear();
        // Invalidate cache since search changed
        self.invalidate_search_cache();
        // Clear match tracking
        self.header_match_uids.clear();
        self.body_match_uids.clear();
        if self.modal.is_search() {
            self.modal = ModalState::None;
        }
        self.selected_thread = 0;
        self.selected_in_thread = 0;
        self.scroll_offset = 0;
    }

    /// Get the match type for a given email UID
    /// Used to show [body] indicator in search results
    pub fn get_match_type(&self, uid: u32) -> MatchType {
        let in_headers = self.header_match_uids.contains(&uid);
        let in_body = self.body_match_uids.contains(&uid);
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
        if self.search_query.is_empty() {
            return HashSet::new();
        }

        let query_lower = self.search_query.to_lowercase();
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
        self.header_match_uids = header_matches.clone();
        self.body_match_uids = body_matches.clone();

        if self.search_query.is_empty() && self.view_mode == ViewMode::All {
            self.cached_visible_indices = Some((0..self.threads.len()).collect());
            self.cached_search_query = self.search_query.clone();
            self.cached_view_mode = self.view_mode;
            return;
        }

        // Merge: thread visible if ANY email matches headers OR body
        let indices: Vec<usize> = self
            .threads
            .iter()
            .enumerate()
            .filter(|(_, thread)| {
                let matches_search = self.search_query.is_empty()
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

        self.cached_visible_indices = Some(indices);
        self.cached_search_query = self.search_query.clone();
        self.cached_view_mode = self.view_mode;
    }

    /// Get maximum reader scroll value based on current content
    pub fn max_reader_scroll(&self) -> usize {
        if let Some(ref body) = self.current_body {
            // Count lines in content, allow scrolling to last line
            body.display_text().lines().count().saturating_sub(1)
        } else {
            0
        }
    }

    /// Scroll reader down by one line (bounded)
    pub fn scroll_reader_down(&mut self) {
        let max = self.max_reader_scroll();
        if self.reader_scroll < max {
            self.reader_scroll += 1;
        }
    }

    /// Scroll reader up by one line
    pub fn scroll_reader_up(&mut self) {
        self.reader_scroll = self.reader_scroll.saturating_sub(1);
    }

    /// Scroll reader by delta (bounded)
    pub fn scroll_reader_by(&mut self, delta: i32) {
        let max = self.max_reader_scroll();
        let new_scroll = (self.reader_scroll as i32 + delta).clamp(0, max as i32);
        self.reader_scroll = new_scroll as usize;
    }

    /// Reset reader scroll when changing emails
    pub fn reset_reader_scroll(&mut self) {
        self.reader_scroll = 0;
    }

    /// Check if we need to load more emails (user is near the bottom of the list)
    pub fn needs_more_emails(&self) -> bool {
        if self.all_emails_loaded || self.loading {
            return false;
        }
        // Load more when within 20 threads of the end
        let visible = self.visible_threads();
        let threshold = 20;
        self.selected_thread + threshold >= visible.len()
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
        let current_thread = match visible.get(self.selected_thread) {
            Some(t) => *t,
            None => return uids,
        };

        // If thread is expanded, add adjacent emails within the thread
        if self.is_thread_expanded(&current_thread.id) {
            let in_thread = self.selected_in_thread;
            for offset in 1..=radius {
                // Email below in thread
                if in_thread > 0 && in_thread - 1 + offset < current_thread.len() {
                    if let Some(email) =
                        current_thread.email_at(&self.emails, in_thread - 1 + offset)
                    {
                        add_uid(email.uid);
                    }
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
            if self.selected_thread + offset < visible.len() {
                add_uid(
                    visible[self.selected_thread + offset]
                        .latest(&self.emails)
                        .uid,
                );
            }
            // Thread above
            if self.selected_thread >= offset {
                add_uid(
                    visible[self.selected_thread - offset]
                        .latest(&self.emails)
                        .uid,
                );
            }
        }

        uids
    }
}

pub fn render(frame: &mut Frame, state: &AppState) {
    match &state.view {
        View::Inbox => render_inbox(frame, state),
        View::Reader { uid } => render_reader(frame, state, *uid),
        View::Composer { email, field } => render_composer(frame, state, email, *field),
        View::AddAccount { step, data } => render_add_account(frame, state, step, data),
        View::Contacts => render_contacts(frame, state),
    }
}
