# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Bltz is a fast, secure terminal email client written in Rust. It uses an async architecture with Tokio, a TUI built on Ratatui, and supports multiple email accounts with OAuth2 and password authentication.

## Build Commands

```bash
cargo build              # Development build
cargo build --release    # Optimized release build
cargo test               # Run all tests
cargo test <test_name>   # Run a specific test
cargo clippy             # Lint the codebase
cargo check              # Type-check without building
```

## Running

```bash
cargo run                # Start the email client
cargo run -- setup       # Run account setup wizard
cargo run -- help        # Show usage
RUST_LOG=debug cargo run # Run with debug logging
```

Logs are written to `~/.config/bltz/bltz.log`.

## Architecture

### Core Components

- **app/** - Application state and event loop coordination. `App` is the central coordinator that manages UI state, IMAP events, and user input.
- **account/** - Multi-account management. `AccountManager` spawns IMAP actors for each account; `AccountHandle` holds per-account state.
- **mail/imap.rs** - IMAP actor (async task per account). Communicates via channels, handles IDLE, syncs emails, manages flags.
- **mail/smtp.rs** - SMTP client for sending emails.
- **mail/thread.rs** - Email threading algorithm (Message-ID and subject-based).
- **cache/db.rs** - SQLite cache with r2d2 connection pooling. Uses WAL mode.
- **ui/** - Ratatui-based rendering: `inbox.rs` (thread list), `reader.rs` (email view), `composer.rs` (compose), `status_bar.rs`.
- **input/** - Keyboard handling with Vim and Arrow keybinding modes.
- **credentials.rs** - System keyring integration for secure credential storage.
- **oauth2.rs** - OAuth2 device code flow for Gmail.

### Data Flow

```
User Input → Input Handler → Action Processing → IMAP Actor Commands
                                    ↓
                           Cache Updates (SQLite)
                                    ↓
                           UI State Updates → Ratatui Rendering
```

### Key Patterns

- **Actor model**: Each email account has its own async IMAP actor communicating via `tokio::sync::mpsc` channels.
- **Prefetching**: Debounced (150ms) background fetching of email bodies for selected and nearby emails.
- **Pagination**: Email lists paginate at 500 emails per page.
- **Optimistic UI**: Flag changes update UI immediately before server confirmation.

## Configuration

- Config file: `~/.config/bltz/config.toml`
- Cache database: `~/.local/share/bltz/cache.db`
- Credentials: System keyring (or `BLTZ_PASSWORD` env var fallback)

## Keybindings (Vim mode - default)

| Key | Action |
|-----|--------|
| j/k | Navigate up/down |
| h/l | Collapse/expand thread |
| g/G | Go to top/bottom |
| Ctrl+d/u | Page down/up |
| Enter | Open email |
| q | Quit |
| r | Reply |
| a | Reply all |
| f | Forward |
| c | Compose |
| d | Delete |
| m | Toggle read/unread |
| s | Toggle star |
| S | View starred |
| Ctrl+r | Refresh |
| Tab/Space | Toggle thread |
| [ / ] | Switch accounts |
| b | Folder picker |
| B | Contacts |
| / | Search |
| : | Command mode |
| . | Help (keybindings + commands) |
| u | Undo |
| C | Toggle conversation view |

### View Modes (Vim mode)

| Key | Action | Description |
|-----|--------|-------------|
| C | Toggle conversation view | Show/hide sent emails in inbox threads |
| S | View starred | Toggle starred-only view |

### AI Features (Vim mode)

| Key | Action | Context |
|-----|--------|---------|
| T | Toggle AI summary | Reader |
| Ctrl+t | Summarize thread | Reader |
| Ctrl+p | Polish text | Composer |
| Enter | Accept polish | Polish preview |
| Esc | Reject polish | Polish preview |

## UI Configuration

```toml
[ui]
keybinding_mode = "vim"      # "vim" or "arrows"
conversation_mode = true     # Show sent emails in inbox threads (default: true)
split_ratio = 50             # Inbox/preview split ratio (30-70)
```

## AI Configuration

AI features are optional and disabled by default. To enable, add to `config.toml`:

```toml
[ai]
api_key = "sk-or-v1-..."        # OpenRouter API key (required)
enable_summarization = true      # Enable email/thread summarization
enable_polish = true             # Enable grammar polish in composer
model = "anthropic/claude-3-haiku"  # Model to use (default)
```

- **ai/** - AI actor module for OpenRouter integration. Follows the same actor pattern as IMAP with async channels.
