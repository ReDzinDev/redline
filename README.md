# Redline

Lightweight Windows writing assistant for inline grammar checks and AI rewrites in any app.

> Status: Early development (Phase 1 — Milestone 0 scaffold).

## Privacy

Redline's own code only talks to OpenRouter, and only when you run an AI action, test your key,
or add a custom model. Offline English grammar checking runs locally with Harper and requires no
API key.

Notes:

- The Windows WebView2 runtime itself may contact Microsoft for runtime updates.
- Your OpenRouter API key is stored in Windows Credential Manager (which protects against
  config-file leaks, not malware running as the same user).

## Prerequisites (Build from Source)

- Windows 10/11
- Rust (stable toolchain; `rust-toolchain.toml` pins stable with `rustfmt` and `clippy`)
- Visual Studio Build Tools with the **Desktop development with C++** workload
- Node.js 24+ and `pnpm`

## Development

```powershell
pnpm install
pnpm tauri dev
```

## Checks

```powershell
# Frontend
pnpm check
pnpm test
pnpm build

# Rust
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

## License

MIT — see [LICENSE](LICENSE).
