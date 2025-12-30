pub mod imap;
pub mod parser;
pub mod smtp;
pub mod thread;
pub mod types;

pub use imap::{
    ImapActorHandle, ImapClient, ImapCommand, ImapEvent, folder_cache_key, spawn_imap_actor,
};
pub use smtp::SmtpClient;
pub use thread::{EmailThread, ThreadId, group_into_threads};
