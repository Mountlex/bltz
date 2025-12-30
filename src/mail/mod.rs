pub mod imap;
pub mod parser;
pub mod smtp;
pub mod thread;
pub mod types;

pub use imap::{
    folder_cache_key, spawn_imap_actor, ImapActorHandle, ImapClient, ImapCommand, ImapEvent,
};
pub use smtp::SmtpClient;
pub use thread::{group_into_threads, EmailThread, ThreadId};
