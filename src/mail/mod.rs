pub mod imap;
pub mod parser;
pub mod smtp;
pub mod thread;
pub mod types;

#[allow(unused_imports)]
pub use imap::{
    FolderMonitorEvent, FolderMonitorHandle, ImapActorHandle, ImapClient, ImapCommand, ImapError,
    ImapEvent, folder_cache_key, spawn_folder_monitor, spawn_imap_actor,
};
pub use smtp::SmtpClient;
pub use thread::{EmailThread, ThreadId, group_into_threads, merge_into_threads};
