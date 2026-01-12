//! Add account wizard UI rendering

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::theme::{Theme, borders};
use crate::app::state::{AddAccountAuth, AddAccountData, AddAccountStep, AppState};

/// Get the current step number and total steps for progress display
fn step_progress(step: &AddAccountStep) -> (u8, u8) {
    const TOTAL_STEPS: u8 = 6;
    let current = match step {
        AddAccountStep::ChooseAuthMethod => 1,
        AddAccountStep::EnterEmail => 2,
        AddAccountStep::EnterPassword | AddAccountStep::OAuth2Flow => 3,
        AddAccountStep::EnterImapServer => 4,
        AddAccountStep::EnterSmtpServer => 5,
        AddAccountStep::Confirm => 6,
    };
    (current, TOTAL_STEPS)
}

pub fn render_add_account(
    frame: &mut Frame,
    _state: &AppState,
    step: &AddAccountStep,
    data: &AddAccountData,
) {
    let area = frame.area();

    // Create a centered dialog box
    let dialog_width = 60.min(area.width.saturating_sub(4));
    let dialog_height = 16.min(area.height.saturating_sub(4));

    let dialog_area = Rect {
        x: (area.width - dialog_width) / 2,
        y: (area.height - dialog_height) / 2,
        width: dialog_width,
        height: dialog_height,
    };

    // Clear the dialog area
    frame.render_widget(Clear, dialog_area);

    // Render the dialog border with step progress
    let (current_step, total_steps) = step_progress(step);
    let title = format!(" Add Account [{}/{}] ", current_step, total_steps);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(borders::popup())
        .border_style(Theme::border_focused());

    let inner_area = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    // Render content based on current step
    let content = match step {
        AddAccountStep::ChooseAuthMethod => render_auth_method_choice(data),
        AddAccountStep::EnterEmail => render_email_input(data),
        AddAccountStep::EnterPassword => render_password_input(data),
        AddAccountStep::OAuth2Flow => render_oauth2_flow(data),
        AddAccountStep::EnterImapServer => render_imap_input(data),
        AddAccountStep::EnterSmtpServer => render_smtp_input(data),
        AddAccountStep::Confirm => render_confirm(data),
    };

    frame.render_widget(content, inner_area);
}

fn render_auth_method_choice(data: &AddAccountData) -> Paragraph<'static> {
    let selected = match data.auth_method {
        AddAccountAuth::Password => 0,
        AddAccountAuth::OAuth2Gmail => 1,
    };

    let mut lines = vec![Line::from("Choose authentication method:"), Line::from("")];

    let options = [
        ("Password", "Traditional password authentication"),
        ("OAuth2 (Gmail)", "Google OAuth2 device flow"),
    ];

    for (i, (name, desc)) in options.iter().enumerate() {
        let prefix = if i == selected { "> " } else { "  " };
        let style = if i == selected {
            Theme::input_highlight()
        } else {
            Theme::text()
        };
        lines.push(Line::styled(format!("{}{}", prefix, name), style));
        lines.push(Line::styled(format!("    {}", desc), Theme::text_muted()));
    }

    lines.push(Line::from(""));
    lines.push(Line::styled(
        "↑/↓ to select, Enter to continue, Esc to cancel",
        Theme::text_muted(),
    ));

    Paragraph::new(lines)
}

fn render_email_input(data: &AddAccountData) -> Paragraph<'static> {
    let email_display = if data.email.is_empty() {
        "_".to_string()
    } else {
        format!("{}_", data.email)
    };

    let lines = vec![
        Line::from("Enter your email address:"),
        Line::from(""),
        Line::styled(email_display, Theme::input_highlight()),
        Line::from(""),
        Line::from(""),
        Line::styled(
            "Type your email, then press Enter to continue",
            Theme::text_muted(),
        ),
        Line::styled("Press Esc to go back", Theme::text_muted()),
    ];

    Paragraph::new(lines)
}

fn render_password_input(data: &AddAccountData) -> Paragraph<'static> {
    let password_display = if data.password.is_empty() {
        "_".to_string()
    } else {
        format!("{}_", "*".repeat(data.password.len()))
    };

    let lines = vec![
        Line::from(format!("Enter password for {}:", data.email)),
        Line::from(""),
        Line::styled(password_display, Theme::input_highlight()),
        Line::from(""),
        Line::from(""),
        Line::styled(
            "Type your password, then press Enter to continue",
            Theme::text_muted(),
        ),
        Line::styled("Press Esc to go back", Theme::text_muted()),
    ];

    Paragraph::new(lines)
}

fn render_oauth2_flow(data: &AddAccountData) -> Paragraph<'static> {
    let mut lines = vec![Line::from("Google OAuth2 Authorization"), Line::from("")];

    if let Some(ref code) = data.oauth2_user_code {
        lines.push(Line::from("1. Visit this URL in your browser:"));
        lines.push(Line::from(""));
        lines.push(Line::styled(
            data.oauth2_url.clone().unwrap_or_default(),
            Theme::text_link(),
        ));
        lines.push(Line::from(""));
        lines.push(Line::from("2. Enter this code:"));
        lines.push(Line::from(""));
        lines.push(Line::styled(code.clone(), Theme::input_highlight()));
        lines.push(Line::from(""));

        if let Some(ref status) = data.oauth2_status {
            lines.push(Line::styled(status.clone(), Theme::text_success()));
        } else {
            lines.push(Line::styled(
                "Waiting for authorization...",
                Theme::text_muted(),
            ));
        }
    } else {
        lines.push(Line::styled("Starting OAuth2 flow...", Theme::text_muted()));
    }

    lines.push(Line::from(""));
    lines.push(Line::styled("Press Esc to cancel", Theme::text_muted()));

    Paragraph::new(lines).wrap(Wrap { trim: false })
}

fn render_imap_input(data: &AddAccountData) -> Paragraph<'static> {
    let server_display = if data.imap_server.is_empty() {
        "_".to_string()
    } else {
        format!("{}_", data.imap_server)
    };

    // Try to suggest IMAP server from email domain
    let suggestion = if data.imap_server.is_empty() {
        if let Some(domain) = data.email.split('@').nth(1) {
            if domain.contains("gmail") {
                Some("imap.gmail.com")
            } else {
                Some(&format!("imap.{}", domain) as &str)
            }
        } else {
            None
        }
    } else {
        None
    };

    let mut lines = vec![
        Line::from("Enter IMAP server:"),
        Line::from(""),
        Line::styled(server_display, Theme::input_highlight()),
    ];

    if let Some(suggested) = suggestion {
        lines.push(Line::from(""));
        lines.push(Line::styled(
            format!("Suggested: {}", suggested),
            Theme::text_muted(),
        ));
    }

    lines.push(Line::from(""));
    lines.push(Line::styled(
        "Press Enter to continue, Esc to go back",
        Theme::text_muted(),
    ));

    Paragraph::new(lines)
}

fn render_smtp_input(data: &AddAccountData) -> Paragraph<'static> {
    let server_display = if data.smtp_server.is_empty() {
        "_".to_string()
    } else {
        format!("{}_", data.smtp_server)
    };

    // Try to suggest SMTP server from email domain
    let suggestion = if data.smtp_server.is_empty() {
        if let Some(domain) = data.email.split('@').nth(1) {
            if domain.contains("gmail") {
                Some("smtp.gmail.com")
            } else {
                Some(&format!("smtp.{}", domain) as &str)
            }
        } else {
            None
        }
    } else {
        None
    };

    let mut lines = vec![
        Line::from("Enter SMTP server:"),
        Line::from(""),
        Line::styled(server_display, Theme::input_highlight()),
    ];

    if let Some(suggested) = suggestion {
        lines.push(Line::from(""));
        lines.push(Line::styled(
            format!("Suggested: {}", suggested),
            Theme::text_muted(),
        ));
    }

    lines.push(Line::from(""));
    lines.push(Line::styled(
        "Press Enter to continue, Esc to go back",
        Theme::text_muted(),
    ));

    Paragraph::new(lines)
}

fn render_confirm(data: &AddAccountData) -> Paragraph<'static> {
    let auth_str = match data.auth_method {
        AddAccountAuth::Password => "Password",
        AddAccountAuth::OAuth2Gmail => "OAuth2 (Gmail)",
    };

    let lines = vec![
        Line::from("Confirm account details:"),
        Line::from(""),
        Line::styled(format!("  Email:   {}", data.email), Theme::text()),
        Line::styled(format!("  Auth:    {}", auth_str), Theme::text()),
        Line::styled(format!("  IMAP:    {}", data.imap_server), Theme::text()),
        Line::styled(format!("  SMTP:    {}", data.smtp_server), Theme::text()),
        Line::from(""),
        Line::from(""),
        Line::styled(
            "Press Enter to add account, Esc to cancel",
            Theme::text_success(),
        ),
    ];

    Paragraph::new(lines)
}
