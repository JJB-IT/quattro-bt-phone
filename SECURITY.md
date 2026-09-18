# Security policy

This project handles your phone calls, contacts and call recordings. Please report
vulnerabilities privately via
[GitHub security advisories](https://github.com/JJB-IT/quattro-bt-phone/security/advisories/new)
rather than public issues.

Design commitments:
- The daemon listens only on a Unix socket in `$XDG_RUNTIME_DIR` (mode `0600`). It opens no network ports.
- Access to the phone's phonebook (PBAP) is read-only.
- Contacts, history and recordings stay on your machine under `~/.local/share/quattro-bt-phone/`.
