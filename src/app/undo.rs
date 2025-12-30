//! Undo system data structures
//!
//! Supports undo for:
//! - Toggle read/unread
//! - Toggle star
//! - Delete (with delayed execution)

use std::time::Instant;

use crate::mail::types::EmailHeader;

/// Represents an action that can be undone
#[derive(Debug, Clone)]
pub enum UndoableAction {
    /// Toggle read was performed - stores uid and the PREVIOUS state (before toggle)
    ToggleRead { uid: u32, was_seen: bool },
    /// Toggle star was performed - stores uid and the PREVIOUS state
    ToggleStar { uid: u32, was_flagged: bool },
    /// Delete was performed - stores the full email header for restoration
    Delete {
        email: Box<EmailHeader>,
        /// When the delete was initiated (for delayed execution)
        initiated_at: Instant,
        /// Index in threads where email was (for restoring selection)
        thread_index: usize,
    },
}

/// Entry in the undo stack
#[derive(Debug, Clone)]
pub struct UndoEntry {
    pub action: UndoableAction,
    pub account_id: String,
    pub folder: String,
}

/// A deletion that is scheduled but not yet executed
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PendingDeletion {
    pub uid: u32,
    pub email: EmailHeader,
    pub initiated_at: Instant,
    pub account_id: String,
    pub folder: String,
}
