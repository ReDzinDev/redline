# Redline

Lightweight Windows writing assistant for instant offline grammar checks and AI rewrites in any app.

Select text anywhere on Windows, press **`Ctrl+Alt+G`**, fix typos offline with [Harper](https://writewithharper.com) or run an AI rewrite via [OpenRouter](https://openrouter.ai), inspect the word-level diff, and press **`Enter`** to replace the text in place — without disturbing your clipboard or Win+V history.

> **Demo GIF placeholder:** `docs/demo.gif` — to be recorded after the visual design pass.

## Features

- **Inline popup in any Windows app** — reads the current selection via Windows UI Automation first (without touching your clipboard) and falls back to an atomic clipboard snapshot & delayed-render paste that preserves images, files, rich formats, and keeps Win+V history clean.
- **Instant offline English grammar checking (no API key required)** — powered by `harper-core` with distinct underline styles (wavy spelling, solid grammar, dotted style), one-click Apply, Fix all (`F`), Ignore, undo (`Ctrl+Z`), and American / British / Canadian / Australian / Indian English dialects.
- **6 streaming AI actions via OpenRouter** — `1` Improve, `2` Fix grammar, `3` Shorten, `4` Clearer, `5` Tone (Professional / Friendly / Casual / Confident), and `6` Custom (remembers your last 5 instructions).
- **Word-level Diff view (`D`)** — compare original vs. rewritten text before pasting. Complete-only accept rules ensure cut-off or failed streams never overwrite your selection.
- **Main window (`Write`, `History`, `Settings`)** — longer scratchpad editor (up to 40,000 characters), last 20 rewrites & grammar fixes with expandable diffs and an off switch, and full settings including press-to-record global hotkey and optional start at login.
- **Lightweight by design** — lives quietly in the system tray; the hidden popup drops into WebView2 low-memory mode and the main window is destroyed when closed.

## Install & Build from Source

Unsigned release builds (Phase 1 `v0.1.0`) will be attached to GitHub Releases. Because Phase 1 builds are unsigned, Windows SmartScreen may prompt before running the binary.

### Prerequisites

- Windows 10 or Windows 11 (x64)
- [Rust](https://rustup.rs/) stable toolchain (`rust-toolchain.toml` pins `stable` with `rustfmt` and `clippy`)
- [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the **Desktop development with C++** workload (uses Windows' built-in Schannel TLS — no NASM or CMake required)
- [Node.js](https://nodejs.org/) 24+ and [`pnpm`](https://pnpm.io/) (`pnpm@12`)

### Run locally

```powershell
pnpm install
pnpm tauri dev
```

### Build a release binary

```powershell
pnpm tauri build
```

## OpenRouter Setup (Optional)

Offline English grammar checks work immediately with no account or key. To enable AI actions (`1`–`6`):

1. Create an API key at [openrouter.ai/keys](https://openrouter.ai/keys) (setting a small spending limit on the key is recommended).
2. Open Redline from the system tray → **Settings** (or enter it in the first-launch Welcome screen).
3. Paste your key, click **Save** (stored in Windows Credential Manager, never written to a JSON config file), and click **Test** to confirm validity and see the key's remaining limit.

### Curated Models

| Model                                    | USD / 1M tokens (in / out) | Notes                         |
| ---------------------------------------- | -------------------------- | ----------------------------- |
| `anthropic/claude-haiku-5.5` _(default)_ | `$0.10` / `$0.50`          | Fast, strong at minimal edits |
| `openai/gpt-6-luna`                      | `$0.10` / `$0.50`          | OpenAI fast GPT-6 tier        |
| `google/gemini-2.5-flash-lite`           | `$0.10` / `$0.40`          | Low-cost Gemini               |
| `deepseek/deepseek-v4.1-flash`           | `$0.14` / `$0.42`          | Fast general-purpose rewriter |
| `qwen/qwen3.7-flash`                     | `$0.03` / `$0.13`          | Lowest cost overall           |

You can also enter any OpenRouter **Custom model ID** in Settings; Redline verifies the ID against OpenRouter's model catalog once to confirm it exists and disable unnecessary reasoning tokens.

## Privacy & Security

**Redline's own code only talks to OpenRouter, and only when you run an AI action, test your key, or verify a custom model ID.** There is no telemetry, analytics, or update phone-home in Redline.

How this is enforced — and the honest caveats:

- **Webview lockdown:** Redline applies an explicit CSP (`default-src 'self'`, `connect-src ipc: http://ipc.localhost`), blocks all non-app navigation in Rust (`on_navigation`), and restricts external link opening strictly to fixed URLs (`repo`, OpenRouter keys/credits) from the main window only.
- **No user text in logs:** Application logs (size-capped to ~1 MB × 2 files, accessible via **Settings › About › Open log folder**) record only timings, character counts, error codes, model IDs, and process names — never your text, prompts, AI results, or API keys.
- **Provider training opt-out:** By default, Redline sends `provider: { data_collection: "deny" }` so OpenRouter excludes providers that retain and train on your text. (Note: `deny` excludes training/retention providers, which is distinct from strict Zero Data Retention.)
- **WebView2 runtime caveat:** The Windows WebView2 runtime itself is managed by Microsoft and may contact Microsoft servers for runtime updates.
- **Credential Manager threat model:** Storing your API key in Windows Credential Manager prevents accidental config-file leaks, but cannot protect secrets from malicious software already running under the same Windows user account.
- **Cancelling in-flight requests:** Pressing `Esc` or closing the popup drops the HTTP connection immediately. OpenRouter stops billing cancelled requests on Anthropic and OpenAI providers, but upstream providers such as Google, Groq, and Mistral may still bill the in-flight request.
- **Local history:** Saved locally in your Windows AppData folder (last 20 entries) and can be cleared or turned off completely in Settings.

To report a security issue privately, see [SECURITY.md](SECURITY.md).

## Keyboard Reference

| Context             | Shortcut                      | Action                                                                |
| ------------------- | ----------------------------- | --------------------------------------------------------------------- |
| Anywhere on Windows | `Ctrl+Alt+G` _(configurable)_ | Capture selection and open inline popup (or toggle closed)            |
| Popup               | `1`–`6`                       | Run AI action (`5` opens tone chips `1`–`4`; `6` opens custom prompt) |
| Popup               | `F`                           | Fix all offline grammar suggestions                                   |
| Popup               | `D`                           | Toggle Result / Diff view                                             |
| Popup               | `Enter`                       | Paste complete result or applied grammar fixes; otherwise close       |
| Popup               | `Esc`                         | Close tone/custom picker first, then close popup and return focus     |
| Popup               | `Ctrl+E`                      | Move current text to the Write tab                                    |
| Popup               | `Ctrl+Z`                      | Undo last grammar fix or Fix all                                      |
| Write tab           | `Ctrl+1`–`Ctrl+6`             | Run AI action on editor contents                                      |

## Roadmap (Phase 2)

- Visual design pass (final palette, typography, spacing, and iconography via `src/styles/tokens.css`)
- Personal dictionary and per-rule grammar toggles
- Optional Zero Data Retention (`zdr`) provider filter
- Additional direct providers (OpenAI, Groq, Gemini, Ollama, LM Studio) on the existing `ChatClient`
- Signed Windows installer and auto-update channel
- Secondary hotkey and multi-turn refinement chips

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for local commands, testing guidelines, and pull request expectations, and [THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md) for dependency licenses.

## License

MIT — see [LICENSE](LICENSE).
