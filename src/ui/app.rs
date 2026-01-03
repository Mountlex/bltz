//! UI rendering dispatch
//!
//! This module contains only the render dispatch function.
//! All state types live in `crate::app::state`.

use ratatui::Frame;

use crate::app::state::{AppState, View};

use super::add_account::render_add_account;
use super::composer::render_composer;
use super::contacts::render_contacts;
use super::inbox::render_inbox;
use super::reader::render_reader;

pub fn render(frame: &mut Frame, state: &AppState) {
    match &state.view {
        View::Inbox => render_inbox(frame, state),
        View::Reader { uid } => render_reader(frame, state, *uid),
        View::Composer { email, field } => render_composer(frame, state, email, *field),
        View::AddAccount { step, data } => render_add_account(frame, state, step, data),
        View::Contacts => render_contacts(frame, state),
    }
}
