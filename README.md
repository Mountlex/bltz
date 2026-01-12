# Bltz

A fast terminal email client written in Rust.

[![CI](https://github.com/Mountlex/bltz/actions/workflows/ci.yml/badge.svg)](https://github.com/Mountlex/bltz/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.92%2B-orange.svg)](https://www.rust-lang.org/)

## Screenshots

<!-- TODO: Add screenshots -->
*Screenshots coming soon*

## Features

- **Multi-account support** - Manage multiple email accounts simultaneously
- **OAuth2 & password authentication** - Secure login with Gmail OAuth2 device flow or traditional passwords
- **Vim-style keybindings** - Navigate efficiently with familiar Vim motions (Arrow key mode also available)
- **Email threading** - Conversations grouped intelligently by Message-ID and subject
- **Fast full-text search** - Search across all your emails
- **Offline caching** - SQLite-backed cache for fast access
- **Desktop notifications** - Get notified of new emails
- **Secure credential storage** - Passwords stored in system keyring
- **AI features** (optional) - Email summarization and grammar polish via OpenRouter

## Installation

### Pre-built Binaries

Download the latest release from the [Releases page](https://github.com/Mountlex/bltz/releases).

```bash
# Linux x86_64
curl -LO https://github.com/Mountlex/bltz/releases/latest/download/bltz-linux-x86_64.tar.gz
tar xzf bltz-linux-x86_64.tar.gz
sudo mv bltz /usr/local/bin/

# macOS x86_64
curl -LO https://github.com/Mountlex/bltz/releases/latest/download/bltz-macos-x86_64.tar.gz
tar xzf bltz-macos-x86_64.tar.gz
sudo mv bltz /usr/local/bin/
```

### Cargo Install

```bash
cargo install bltz
```

### Build from Source

```bash
git clone https://github.com/Mountlex/bltz.git
cd bltz
cargo build --release
./target/release/bltz
```

### Package Managers

<!-- TODO: Add when available -->
- **Homebrew**: `brew install bltz` *(coming soon)*
- **AUR**: `yay -S bltz` *(coming soon)*

## Configuration

Bltz stores its configuration at `~/.config/bltz/config.toml`.

### Initial Setup

Run the setup wizard to configure your first account:

```bash
bltz setup
```

### Manual Configuration

Create `~/.config/bltz/config.toml`:

```toml
[general]
keybindings = "vim"  # or "arrows"
theme = "dark"       # or "light"

[[accounts]]
name = "Personal"
email = "you@gmail.com"
imap_host = "imap.gmail.com"
imap_port = 993
smtp_host = "smtp.gmail.com"
smtp_port = 587
auth = "oauth2"      # or "password"
```

See [config.example.toml](config.example.toml) for a complete example.

### AI Features (Optional)

AI features are disabled by default. To enable email summarization and grammar polish:

```toml
[ai]
api_key = "sk-or-v1-..."           # OpenRouter API key
enable_summarization = true
enable_polish = true
model = "anthropic/claude-3-haiku" # Optional, this is the default
```

## Usage

```bash
bltz          # Start the email client
bltz setup    # Run account setup wizard
bltz help     # Show usage information
```

### Debug Mode

```bash
RUST_LOG=debug bltz
```

Logs are written to `~/.config/bltz/bltz.log`.

## Keybindings

### Navigation (Vim Mode)

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate down / up |
| `h` / `l` | Collapse / expand thread |
| `g` / `G` | Go to top / bottom |
| `Ctrl+d` / `Ctrl+u` | Page down / up |
| `Enter` | Open email |
| `Esc` | Back |
| `q` | Quit |
| `Tab` / `Space` | Toggle thread |
| `[` / `]` | Switch accounts |
| `b` | Folder picker |
| `/` | Search |
| `:` | Command mode |
| `.` | Help |

### Actions

| Key | Action |
|-----|--------|
| `c` | Compose new email |
| `r` | Reply |
| `a` | Reply all |
| `f` | Forward |
| `d` | Delete |
| `m` | Toggle read / unread |
| `s` | Toggle star |
| `S` | View starred emails |
| `C` | Toggle conversation view |
| `Ctrl+r` | Refresh |
| `u` | Undo |
| `B` | Contacts |
| `H` | Expand/collapse headers |

### Attachments

| Key | Action | Context |
|-----|--------|---------|
| `A` | Toggle attachment list | Reader |
| `j` / `k` | Navigate attachments | Attachment list |
| `Enter` | Open with system app | Attachment list |
| `s` | Save to ~/Downloads | Attachment list |
| `Esc` | Close attachment list | Attachment list |

### AI Features

| Key | Action | Context |
|-----|--------|---------|
| `T` | Toggle AI summary | Reader |
| `Ctrl+t` | Summarize thread | Reader |
| `Ctrl+p` | Polish text | Composer |
| `Enter` | Accept polish | Polish preview |
| `Esc` | Reject polish | Polish preview |

## File Locations

| File | Location |
|------|----------|
| Configuration | `~/.config/bltz/config.toml` |
| Log file | `~/.config/bltz/bltz.log` |
| Cache database | `~/.local/share/bltz/cache.db` |
| Credentials | System keyring |

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## Security

For security vulnerabilities, please see [SECURITY.md](SECURITY.md).

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
