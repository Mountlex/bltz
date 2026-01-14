//! AI feature action handlers

use crate::ai::AiCommand;
use crate::app::state::{PolishPreview, View};

use super::super::App;

impl App {
    /// Toggle between full email and AI summary in reader view
    pub(crate) async fn toggle_summary(&mut self) {
        // Check if AI is enabled
        if self.ai_actor.is_none() {
            self.state.set_error("AI features not configured");
            return;
        }

        if !self.config.ai.enable_summarization {
            self.state
                .set_error("AI summarization is disabled in config");
            return;
        }

        // Works in reader view and inbox view (with preview)
        let uid = match self.state.view {
            View::Reader { uid } => uid,
            View::Inbox => match self.state.current_email_from_thread() {
                Some(email) => email.uid,
                None => return,
            },
            _ => {
                self.state
                    .set_error("Summary toggle only works in reader/inbox view");
                return;
            }
        };

        // Toggle the view mode
        self.state.reader.show_summary = !self.state.reader.show_summary;

        // If switching to summary view and no cached summary, request one
        if self.state.reader.show_summary {
            let needs_fetch = match &self.state.reader.cached_summary {
                Some((cached_uid, _)) => *cached_uid != uid,
                None => true,
            };

            if needs_fetch {
                self.request_email_summary(uid).await;
            }
        }
    }

    /// Request AI summary for an email
    async fn request_email_summary(&mut self, uid: u32) {
        // Extract needed data first to avoid borrow conflicts
        let body_text = match &self.state.reader.body {
            Some(body) => body.display_text(),
            None => {
                self.state.set_error("Email body not loaded yet");
                return;
            }
        };

        let subject = match self.state.emails.iter().find(|e| e.uid == uid) {
            Some(email) => email.subject.clone(),
            None => return,
        };

        let Some(ref ai) = self.ai_actor else { return };

        self.state.reader.summary_loading = true;
        self.dirty = true;
        self.state.set_status("Generating summary...");

        let _ = ai
            .cmd_tx
            .send(AiCommand::SummarizeEmail {
                uid,
                subject,
                body: body_text,
            })
            .await;
    }

    /// Summarize the entire current thread
    pub(crate) async fn summarize_thread(&mut self) {
        // Check if AI is enabled
        if self.ai_actor.is_none() {
            self.state.set_error("AI features not configured");
            return;
        }

        if !self.config.ai.enable_summarization {
            self.state
                .set_error("AI summarization is disabled in config");
            return;
        }

        // Get current thread
        let thread = match self.state.current_thread() {
            Some(t) => t.clone(),
            None => {
                self.state.set_error("No thread selected");
                return;
            }
        };

        if thread.len() < 2 {
            self.state
                .set_error("Thread has only one email, use single email summary");
            return;
        }

        // Check if we have a cached summary for this thread
        if let Some((ref cached_id, _)) = self.state.reader.cached_thread_summary
            && *cached_id == thread.id
        {
            // Already have summary, just toggle view
            self.state.reader.show_summary = true;
            return;
        }

        // Collect thread emails (we need bodies, which may not be cached)
        // For now, collect what we have from headers
        let emails: Vec<(String, String, String)> = thread
            .emails(&self.state.emails)
            .map(|e| {
                let from = e.display_from().to_string();
                let subject = e.subject.clone();
                // Use preview as body approximation if full body not available
                let body = e.preview.clone().unwrap_or_default();
                (from, subject, body)
            })
            .collect();

        let Some(ref ai) = self.ai_actor else { return };

        self.state.reader.summary_loading = true;
        self.dirty = true;
        self.state.reader.show_summary = true;
        self.state.set_status("Generating thread summary...");

        let _ = ai
            .cmd_tx
            .send(AiCommand::SummarizeThread {
                thread_id: thread.id.clone(),
                emails,
            })
            .await;
    }

    /// Start AI polish for composer body text
    pub(crate) async fn start_polish(&mut self) {
        // Check if AI is enabled
        if self.ai_actor.is_none() {
            self.state.set_error("AI features not configured");
            return;
        }

        if !self.config.ai.enable_polish {
            self.state.set_error("AI polish is disabled in config");
            return;
        }

        // Only works in composer view
        let body = match &self.state.view {
            View::Composer { email, .. } => email.body.clone(),
            _ => {
                self.state.set_error("Polish only works in composer");
                return;
            }
        };

        if body.trim().is_empty() {
            self.state.set_error("Nothing to polish");
            return;
        }

        let Some(ref ai) = self.ai_actor else { return };

        // Set up polish preview in loading state
        self.state.polish.preview = Some(PolishPreview {
            original: body.clone(),
            polished: String::new(),
            loading: true,
        });
        self.dirty = true;
        self.state.set_status("Polishing text...");

        let _ = ai.cmd_tx.send(AiCommand::Polish { original: body }).await;
    }

    /// Accept the polished text and apply it to the composer
    pub(crate) fn accept_polish(&mut self) {
        if let Some(preview) = self.state.polish.preview.take()
            && !preview.loading
            && !preview.polished.is_empty()
            && let View::Composer { ref mut email, .. } = self.state.view
        {
            email.body = preview.polished;
            self.state.set_status("Polish applied");
        }
    }

    /// Reject the polished text and keep the original
    pub(crate) fn reject_polish(&mut self) {
        if self.state.polish.preview.is_some() {
            self.state.polish.preview = None;
            self.state.set_status("Polish cancelled");
        }
    }
}
