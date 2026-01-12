# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.3.x   | :white_check_mark: |
| 0.2.x   | :white_check_mark: |
| < 0.2   | :x:                |

## Reporting a Vulnerability

If you discover a security vulnerability in Bltz, please report it responsibly.

### How to Report

**Do not open a public GitHub issue for security vulnerabilities.**

Instead, please email the maintainer directly or use GitHub's private vulnerability reporting feature:

1. Go to the [Security tab](https://github.com/Mountlex/bltz/security) of this repository
2. Click "Report a vulnerability"
3. Provide a detailed description of the vulnerability

### What to Include

- Type of vulnerability (e.g., credential exposure, injection, etc.)
- Steps to reproduce the issue
- Potential impact
- Suggested fix (if any)

### Response Timeline

- **Initial response**: Within 48 hours
- **Status update**: Within 7 days
- **Fix timeline**: Depends on severity, typically within 30 days for critical issues

### Credential Security

Bltz stores credentials in the system keyring. If you discover any issues with credential handling:

- Credential exposure in logs
- Insecure credential storage
- Credential leakage in network traffic

Please report these with high priority.

## Security Best Practices for Users

1. **Use OAuth2 when available** - Prefer OAuth2 authentication over passwords for Gmail
2. **Keep Bltz updated** - Install security updates promptly
3. **Secure your system keyring** - Bltz relies on your system's keyring security
4. **Review AI configuration** - If using AI features, ensure your API key is kept private
