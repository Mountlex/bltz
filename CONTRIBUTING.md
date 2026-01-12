# Contributing to Bltz

Thank you for your interest in contributing to Bltz! This document provides guidelines and instructions for contributing.

## Getting Started

### Prerequisites

- Rust 1.92 or later
- A system keyring (for credential storage)
- An email account for testing

### Development Setup

1. Clone the repository:
   ```bash
   git clone https://github.com/Mountlex/bltz.git
   cd bltz
   ```

2. Build the project:
   ```bash
   cargo build
   ```

3. Run tests:
   ```bash
   cargo test
   ```

4. Run the application:
   ```bash
   cargo run
   ```

### Debug Mode

For development, run with debug logging:

```bash
RUST_LOG=debug cargo run
```

Logs are written to `~/.config/bltz/bltz.log`.

## Code Style

### Formatting

All code must be formatted with `rustfmt`. This is enforced in CI.

```bash
cargo fmt
```

### Linting

Code must pass `clippy` without warnings:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

### Commit Messages

- Use clear, descriptive commit messages
- Start with a verb in the imperative mood (e.g., "Add", "Fix", "Update")
- Keep the first line under 72 characters
- Reference issues when applicable (e.g., "Fix #123")

Examples:
- `Add OAuth2 support for Outlook`
- `Fix thread collapsing in inbox view`
- `Update dependencies to latest versions`

## Architecture Overview

Bltz uses an actor-based architecture with async Tokio tasks:

- **`app/`** - Application state and event loop
- **`account/`** - Multi-account management
- **`mail/`** - IMAP/SMTP clients and email threading
- **`cache/`** - SQLite caching layer
- **`ui/`** - Ratatui-based terminal UI
- **`input/`** - Keyboard handling

### Key Patterns

1. **Actor Model**: Each email account has its own IMAP actor communicating via `tokio::sync::mpsc` channels.

2. **Optimistic UI**: Flag changes update the UI immediately before server confirmation.

3. **Prefetching**: Email bodies are prefetched in the background for smooth navigation.

## Pull Request Process

1. Fork the repository and create a feature branch
2. Make your changes
3. Ensure all checks pass:
   ```bash
   cargo fmt --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test
   ```
4. Submit a pull request with a clear description
5. Address any review feedback

## Reporting Bugs

Use the [bug report template](.github/ISSUE_TEMPLATE/bug_report.md) when reporting issues. Include:

- Bltz version
- Operating system
- Steps to reproduce
- Expected vs actual behavior
- Relevant log output

## Feature Requests

Use the [feature request template](.github/ISSUE_TEMPLATE/feature_request.md) for suggesting new features.

## Releasing (Maintainers)

Releases are automated via GitHub Actions. To create a new release:

1. Update the version in `Cargo.toml`
2. Commit the version bump:
   ```bash
   git add Cargo.toml Cargo.lock
   git commit -m "Bump version to X.Y.Z"
   ```
3. Create and push a version tag:
   ```bash
   git tag vX.Y.Z
   git push origin main
   git push origin vX.Y.Z
   ```

The release workflow will automatically:
- Build binaries for Linux x86_64 and macOS x86_64
- Generate SHA256 checksums
- Create a GitHub release with the binaries attached

## Security

For security vulnerabilities, please see [SECURITY.md](SECURITY.md) for responsible disclosure guidelines.

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
