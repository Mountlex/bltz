#[derive(Debug, Clone, Default)]
pub struct SyncState {
    pub uid_validity: Option<u32>,
    pub uid_next: Option<u32>,
    pub last_sync: Option<i64>,
}

impl SyncState {
    pub fn needs_full_sync(&self, server_uid_validity: u32) -> bool {
        match self.uid_validity {
            Some(local) => local != server_uid_validity,
            None => true,
        }
    }

    pub fn new_messages_start(&self, server_uid_next: u32) -> Option<u32> {
        self.uid_next.map(|local| {
            if server_uid_next > local {
                Some(local)
            } else {
                None
            }
        })?
    }
}
