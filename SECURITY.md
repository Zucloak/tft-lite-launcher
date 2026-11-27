# Security Policy

## Verifying Release Integrity (`SHA256SUMS.txt`)

Every workflow build and tagged GitHub Release publishes a `SHA256SUMS.txt` manifest alongside the compiled binaries (`TFT-Lite-Launcher-windows-x86_64.exe`, macOS, and Linux archives). Always verify the SHA-256 hash before running a downloaded binary.

### Windows (PowerShell)

```powershell
Get-FileHash .\TFT-Lite-Launcher-windows-x86_64.exe -Algorithm SHA256
Get-Content .\SHA256SUMS.txt
```

Confirm that the hexadecimal hash returned by `Get-FileHash` matches the corresponding entry in `SHA256SUMS.txt` (case-insensitive).

### macOS

```bash
shasum -a 256 -c SHA256SUMS.txt --ignore-missing
```

### Linux

```bash
sha256sum -c SHA256SUMS.txt --ignore-missing
```

## Reporting a Vulnerability

Please **do not** open public GitHub Issues or Pull Requests for security vulnerabilities, supply-chain concerns, or privilege-escalation bugs.

Instead, report vulnerabilities privately through GitHub's **Private Vulnerability Reporting** feature under the repository's **Security** tab (`Report a vulnerability`). Please include:

- A clear description of the affected component (e.g., `src/sdk.rs`, `src/keymapper.rs`, or `.github/workflows/release.yml`).
- Steps to reproduce or proof-of-concept details.
- The operating system, architecture, and launcher version tested.

Maintainers will acknowledge private reports within 72 hours and coordinate a fix before public disclosure.
