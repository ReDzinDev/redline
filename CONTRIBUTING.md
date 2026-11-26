# Contributing to Redline

Thank you for your interest in contributing to Redline. The guiding goal for the project is to stay **lightweight and easy to use**.

## Prerequisites

- **OS:** Windows 10 or Windows 11 (x64)
- **Rust:** Stable toolchain (`rust-toolchain.toml` pins `stable` with `rustfmt` and `clippy`)
- **C++ Build Tools:** Visual Studio Build Tools with the **Desktop development with C++** workload (Redline uses Windows Schannel via `reqwest`'s `native-tls` feature, so NASM and CMake are not required)
- **Node.js & pnpm:** Node.js 24+ and `pnpm` (`packageManager` pins `pnpm@12.23.0`)

## Local Development

```powershell
pnpm install
pnpm tauri dev
```

## Automated Checks

Run all checks locally before opening a pull request:

```powershell
# Frontend formatting, lint (oxlint), TypeScript 7 typecheck, unit tests, and Vite bundle
pnpm check
pnpm test
pnpm build

# Rust formatting, Clippy (warnings denied), and unit tests
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --locked
```

To auto-format frontend files and Markdown/JSON docs with `oxfmt`:

```powershell
pnpm fmt
```

### Opt-in Live OpenRouter Model Check

Whenever the curated model list in `src-tauri/src/ai/models.rs` changes, run the ignored live test locally with a low-limit OpenRouter API key (never committed or added to CI):

```powershell
$env:OPENROUTER_API_KEY = "sk-or-v1-..."
cargo test --manifest-path src-tauri/Cargo.toml --locked -- --ignored --nocapture
```

Every curated model must stream to `[DONE]`, finish with `finish_reason == "stop"`, and report `reasoning_tokens == 0`.

## Manual Testing

Run the checklist in [docs/testing.md](docs/testing.md):

- For everyday changes, run the **core 5-app set** (Notepad, Chrome, Slack, Word, VS Code).
- Before releases or changes touching `platform/windows/`, run the **full matrix** including Outlook, Edge, Firefox, Discord, Notion, Windows Terminal, an elevated (Administrator) Notepad, and a mixed-DPI second monitor.

## Architecture & Coding Conventions

- **Privacy first:** Never log user text, prompts, AI results, or API keys in Rust or TypeScript. Log only lengths, timings, error codes, model IDs, and process names.
- **Rust owns keys and network; React owns the screen:** Never return raw API keys over IPC.
- **Design tokens only:** Put colors, spacing, font sizes, and radii in `src/styles/tokens.css` (`@theme inline`) with light and dark values. Avoid hard-coded hex colors or pixel values in React components.
- **Centralized copy:** Put all user-facing strings and error messages in `src/lib/messages.ts`. Rust returns structured error codes via `ApiErrorDto`.
- **Small conventional commits:** Use prefixes like `feat:`, `fix:`, `refactor:`, `test:`, `docs:`, `chore:`, `ci:`. Keep commits small and focused.
