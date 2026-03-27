# Git hooks

This repository ships a **pre-commit** hook that blocks commits when staged files look like local secrets:

- `.env` and `.env.*` (except `.env.example`, `.env.sample`, `.env.template`)
- SSH private key basenames: `id_rsa`, `id_ecdsa`, `id_ed25519`, `id_dsa`
- `*.pem`, `*.p12`, `*.pfx`, `*.jks`, `*.keystore`
- Staged text that contains an OpenSSH / RSA / EC `PRIVATE KEY` PEM header

Ignored paths are also listed in the root [`.gitignore`](../.gitignore).

## Enable (once per clone)

```bash
git config core.hooksPath .githooks
```

Verify the hook is executable (`chmod +x .githooks/pre-commit` if your tooling stripped the bit).

## Bypass (rare)

If you truly must commit a blocked path (discouraged), you can skip hooks with `git commit --no-verify`. Prefer redacted fixtures or names that do not match the rules above.
