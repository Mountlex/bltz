//! Attachment handling actions

use std::path::PathBuf;

use crate::app::App;
use crate::app::state::View;
use crate::mail::ImapCommand;
use crate::mail::parser::parse_attachments;

impl App {
    /// Toggle attachment list visibility in reader view
    pub(super) async fn toggle_attachments(&mut self) {
        // Only works in reader view
        let uid = match self.state.view {
            View::Reader { uid } => uid,
            _ => return,
        };

        // Check if email has attachments
        let email = self.state.emails.iter().find(|e| e.uid == uid);
        if !email.is_some_and(|e| e.has_attachments) {
            return;
        }

        // Toggle attachment view
        if self.state.reader.show_attachments {
            // Closing attachment view
            self.state.reader.show_attachments = false;
        } else {
            // Opening attachment view - load attachments if not already loaded
            if self.state.reader.attachments.is_empty() {
                self.load_attachments(uid).await;
            }
            self.state.reader.show_attachments = true;
            self.state.reader.attachment_selected = 0;
        }
    }

    /// Load attachments for an email
    async fn load_attachments(&mut self, uid: u32) {
        let cache_key = self.cache_key_for_uid(uid);

        // First try to load from cache
        if let Ok(attachments) = self.cache.get_attachments(&cache_key, uid).await
            && !attachments.is_empty()
        {
            self.state.reader.attachments = attachments;
            return;
        }

        // Try to parse from cached raw message
        if let Ok(Some(raw)) = self.cache.get_raw_message(&cache_key, uid).await {
            let attachments = parse_attachments(&raw);
            if !attachments.is_empty() {
                // Cache the attachment metadata
                self.cache
                    .insert_attachments(&cache_key, uid, &attachments)
                    .await
                    .ok();
                self.state.reader.attachments = attachments;
                return;
            }
        }

        // Need to fetch from server - send command and wait for event
        self.state.set_status("Loading attachments...");
        self.state.status.loading = true;

        // Request the first attachment to trigger raw message fetch
        let folder = self.folder_for_uid(uid);
        self.accounts
            .send_command(ImapCommand::FetchAttachment {
                uid,
                folder,
                attachment_index: 0,
            })
            .await
            .ok();
    }

    /// Save selected attachment to disk
    pub(super) async fn save_attachment(&mut self) {
        // Must be in reader view with attachments showing
        if !matches!(self.state.view, View::Reader { .. }) || !self.state.reader.show_attachments {
            return;
        }

        let uid = match self.state.view {
            View::Reader { uid } => uid,
            _ => return,
        };

        let selected = self.state.reader.attachment_selected;
        let attachment = match self.state.reader.attachments.get(selected) {
            Some(a) => a.clone(),
            None => return,
        };

        // Determine save path
        let downloads_dir = dirs::download_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."));
        let save_path = unique_save_path(&downloads_dir, &attachment.filename);

        // Store pending save info
        self.state.reader.pending_attachment_save = Some((selected, save_path.clone()));

        self.state
            .set_status(format!("Saving {}...", attachment.filename));
        self.state.status.loading = true;

        // Request attachment data
        let folder = self.folder_for_uid(uid);
        self.accounts
            .send_command(ImapCommand::FetchAttachment {
                uid,
                folder,
                attachment_index: selected,
            })
            .await
            .ok();
    }

    /// Open selected attachment with system app
    pub(super) async fn open_attachment(&mut self) {
        // Must be in reader view with attachments showing
        if !matches!(self.state.view, View::Reader { .. }) || !self.state.reader.show_attachments {
            return;
        }

        let uid = match self.state.view {
            View::Reader { uid } => uid,
            _ => return,
        };

        let selected = self.state.reader.attachment_selected;
        let attachment = match self.state.reader.attachments.get(selected) {
            Some(a) => a.clone(),
            None => return,
        };

        // Use temp directory for opening
        let temp_dir = std::env::temp_dir().join("bltz_attachments");
        std::fs::create_dir_all(&temp_dir).ok();
        let temp_path = temp_dir.join(&attachment.filename);

        // Store as pending save (we'll open after fetching)
        self.state.reader.pending_attachment_save = Some((selected, temp_path));

        self.state
            .set_status(format!("Opening {}...", attachment.filename));
        self.state.status.loading = true;

        // Request attachment data
        let folder = self.folder_for_uid(uid);
        self.accounts
            .send_command(ImapCommand::FetchAttachment {
                uid,
                folder,
                attachment_index: selected,
            })
            .await
            .ok();
    }

    /// Handle attachment fetched event
    pub(crate) async fn handle_attachment_fetched(
        &mut self,
        uid: u32,
        attachment_index: usize,
        attachment: crate::mail::types::Attachment,
        data: Vec<u8>,
    ) {
        self.state.status.loading = false;

        // Check if this is for a pending save/open operation
        if let Some((pending_index, save_path)) = self.state.reader.pending_attachment_save.take()
            && pending_index == attachment_index
        {
            enum SaveOutcome {
                Opened,
                Saved(PathBuf),
            }

            let filename = attachment.filename.clone();
            let is_temp = save_path.starts_with(std::env::temp_dir());
            let write_result =
                tokio::task::spawn_blocking(move || -> Result<SaveOutcome, String> {
                    std::fs::write(&save_path, &data)
                        .map_err(|e| format!("Failed to save: {}", e))?;
                    if is_temp {
                        open::that(&save_path).map_err(|e| format!("Failed to open: {}", e))?;
                        Ok(SaveOutcome::Opened)
                    } else {
                        Ok(SaveOutcome::Saved(save_path))
                    }
                })
                .await;

            match write_result {
                Ok(Ok(SaveOutcome::Opened)) => {
                    self.state.set_status(format!("Opened {}", filename));
                }
                Ok(Ok(SaveOutcome::Saved(path))) => {
                    self.state
                        .set_status(format!("Saved to {}", path.display()));
                }
                Ok(Err(error)) => {
                    self.state.set_error(error);
                }
                Err(error) => {
                    self.state
                        .set_error(format!("Attachment save task failed: {}", error));
                }
            }
            return;
        }

        // Otherwise this was just loading attachment metadata
        // Update the attachment list
        if !self
            .state
            .reader
            .attachments
            .iter()
            .any(|a| a.filename == attachment.filename)
        {
            self.state.reader.attachments.push(attachment);
        }

        // Check if we now have all attachments from the email
        let uid_matches =
            matches!(self.state.view, View::Reader { uid: current_uid } if current_uid == uid);
        if uid_matches && self.state.reader.attachments.is_empty() {
            // Try to reload from cache now that raw message should be cached
            let cache_key = self.cache_key_for_uid(uid);
            if let Ok(Some(raw)) = self.cache.get_raw_message(&cache_key, uid).await {
                self.state.reader.attachments = parse_attachments(&raw);
            }
        }

        self.state.set_status("");
    }

    /// Handle attachment fetch failed event
    pub(crate) fn handle_attachment_fetch_failed(
        &mut self,
        _uid: u32,
        _index: usize,
        error: String,
    ) {
        self.state.status.loading = false;
        self.state.reader.pending_attachment_save = None;
        self.state
            .set_error(format!("Failed to fetch attachment: {}", error));
    }
}

/// Generate a unique save path by appending numbers to avoid overwriting
fn unique_save_path(base_dir: &std::path::Path, filename: &str) -> PathBuf {
    let path = base_dir.join(filename);
    if !path.exists() {
        return path;
    }

    let stem = std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|s| s.to_str());

    for i in 1..100 {
        let new_name = match ext {
            Some(e) => format!("{} ({}).{}", stem, i, e),
            None => format!("{} ({})", stem, i),
        };
        let new_path = base_dir.join(new_name);
        if !new_path.exists() {
            return new_path;
        }
    }

    // Fallback: append timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    base_dir.join(format!("{}.{}", filename, timestamp))
}
