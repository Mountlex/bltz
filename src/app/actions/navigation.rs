//! Navigation actions (movement, scrolling)

use crate::app::state::View;

use super::super::App;

impl App {
    pub(crate) fn move_up(&mut self) {
        // Handle folder sidebar navigation
        if self.state.folder.sidebar_visible && self.state.folder.sidebar_focused {
            if self.state.folder.sidebar_selected > 0 {
                self.state.folder.sidebar_selected -= 1;
            }
            return;
        }

        match &mut self.state.view {
            View::Inbox => {
                self.state.move_up();
            }
            View::Reader { .. } => {
                if self.state.reader.show_attachments {
                    self.state.reader.attachment_up();
                } else {
                    self.state.reader.scroll_up();
                }
            }
            View::Contacts => {
                self.contacts_move_up();
            }
            View::AddAccount { step, data } => {
                use crate::app::state::{AddAccountAuth, AddAccountStep};
                if matches!(step, AddAccountStep::ChooseAuthMethod) {
                    // Toggle auth method selection
                    data.auth_method = match data.auth_method {
                        AddAccountAuth::Password => AddAccountAuth::OAuth2Gmail,
                        AddAccountAuth::OAuth2Gmail => AddAccountAuth::Password,
                    };
                }
            }
            _ => {}
        }
    }

    pub(crate) fn move_down(&mut self) {
        // Handle folder sidebar navigation
        if self.state.folder.sidebar_visible && self.state.folder.sidebar_focused {
            if self.state.folder.sidebar_selected < self.state.folder.list.len().saturating_sub(1) {
                self.state.folder.sidebar_selected += 1;
            }
            return;
        }

        match &mut self.state.view {
            View::Inbox => {
                self.state.move_down();
            }
            View::Reader { .. } => {
                if self.state.reader.show_attachments {
                    self.state.reader.attachment_down();
                } else {
                    self.state.reader.scroll_down();
                }
            }
            View::Contacts => {
                self.contacts_move_down();
            }
            View::AddAccount { step, data } => {
                use crate::app::state::{AddAccountAuth, AddAccountStep};
                if matches!(step, AddAccountStep::ChooseAuthMethod) {
                    // Toggle auth method selection
                    data.auth_method = match data.auth_method {
                        AddAccountAuth::Password => AddAccountAuth::OAuth2Gmail,
                        AddAccountAuth::OAuth2Gmail => AddAccountAuth::Password,
                    };
                }
            }
            _ => {}
        }
    }

    pub(super) async fn move_left(&mut self) {
        // Handle sidebar focus: unfocus if focused
        if self.state.folder.sidebar_visible && self.state.folder.sidebar_focused {
            // Already in sidebar and pressing left - do nothing (stay in sidebar)
            return;
        }

        match &self.state.view {
            View::Inbox => {
                // If sidebar is visible but not focused, focus it
                if self.state.folder.sidebar_visible {
                    self.state.folder.sidebar_focused = true;
                    return;
                }
                self.state.collapse_or_move_left();
            }
            View::Reader { uid } => {
                let uid = *uid;
                self.state.view = View::Inbox;
                // Try to keep body from cache for smooth transition
                if let Ok(Some(body)) = self.cache.get_email_body(&self.cache_key(), uid).await {
                    self.state.reader.set_body(Some(body));
                    self.prefetch.last_uid = Some(uid);
                } else {
                    self.state.reader.set_body(None);
                }
            }
            View::Composer { .. } => {
                self.state.view = View::Inbox;
                self.state.reader.set_body(None);
            }
            View::Contacts => {
                // Go back to inbox
                self.state.view = View::Inbox;
            }
            View::AddAccount { .. } => {
                // Handled by wizard navigation
            }
        }
    }

    pub(super) fn move_right(&mut self) {
        // Handle sidebar focus: unfocus if focused
        if self.state.folder.sidebar_visible && self.state.folder.sidebar_focused {
            self.state.folder.sidebar_focused = false;
            return;
        }

        if let View::Inbox = &self.state.view {
            self.state.expand_thread();
        }
    }

    pub(super) fn move_page(&mut self, delta: i32) {
        match &self.state.view {
            View::Inbox => {
                for _ in 0..delta.abs() {
                    if delta > 0 {
                        self.state.move_down();
                    } else {
                        self.state.move_up();
                    }
                }
            }
            View::Reader { .. } => {
                self.state.reader.scroll_by(delta);
            }
            _ => {}
        }
    }

    pub(super) fn toggle_thread(&mut self) {
        if matches!(self.state.view, View::Inbox) {
            self.state.toggle_thread_expansion();
        }
    }

    pub(super) fn move_to_top(&mut self) {
        match &self.state.view {
            View::Inbox => {
                self.state.thread.selected = 0;
                self.state.thread.selected_in_thread = 0;
            }
            View::Reader { .. } => self.state.reader.reset_scroll(),
            _ => {}
        }
    }

    pub(super) fn move_to_bottom(&mut self) {
        if let View::Inbox = &self.state.view {
            // Use visible_thread_count to respect search/filter, not total thread count
            let visible_count = self.state.visible_thread_count();
            if visible_count > 0 {
                self.state.thread.selected = visible_count - 1;
                self.state.thread.selected_in_thread = 0;
            }
        }
    }
}
