# Third-Party Licenses

Redline itself is licensed under the [MIT License](LICENSE).

Below is a hand-maintained inventory of Redline's direct runtime and build dependencies. For the full transitive dependency graph, inspect `src-tauri/Cargo.lock` and `pnpm-lock.yaml`.

## Direct Rust Dependencies (`src-tauri/Cargo.toml`)

| Crate                          | License           | Purpose                                                                                     |
| ------------------------------ | ----------------- | ------------------------------------------------------------------------------------------- |
| `harper-core`                  | Apache-2.0        | Offline English grammar and spell-checking engine                                           |
| `keyring-core`                 | MIT OR Apache-2.0 | Cross-platform credential store trait                                                       |
| `windows-native-keyring-store` | MIT OR Apache-2.0 | Windows Credential Manager backend for API key storage                                      |
| `log`                          | MIT OR Apache-2.0 | Logging facade                                                                              |
| `reqwest`                      | MIT OR Apache-2.0 | Streaming HTTP client (using Windows Schannel via `native-tls`)                             |
| `serde` / `serde_json`         | MIT OR Apache-2.0 | JSON serialization for IPC, settings, history, and OpenRouter                               |
| `tauri` / `tauri-build`        | MIT OR Apache-2.0 | Desktop application framework, WebView2 hosting, and tray icon                              |
| `tauri-plugin-autostart`       | MIT OR Apache-2.0 | Optional Windows sign-in launch registration                                                |
| `tauri-plugin-global-shortcut` | MIT OR Apache-2.0 | System-wide hotkey registration                                                             |
| `tauri-plugin-log`             | MIT OR Apache-2.0 | Size-capped rolling log files                                                               |
| `tauri-plugin-opener`          | MIT OR Apache-2.0 | Opening fixed external URLs and the log folder                                              |
| `tauri-plugin-single-instance` | MIT OR Apache-2.0 | Preventing duplicate running instances                                                      |
| `thiserror`                    | MIT OR Apache-2.0 | Typed Rust error definitions                                                                |
| `webview2-com`                 | MIT               | Direct WebView2 COM settings (`AreBrowserAcceleratorKeysEnabled`, low-memory target)        |
| `windows`                      | MIT OR Apache-2.0 | Win32 APIs (UI Automation, clipboard snapshot & delayed rendering, focus, DPI, `SendInput`) |

## Direct Frontend Dependencies (`package.json`)

| Package               | License           | Purpose                                         |
| --------------------- | ----------------- | ----------------------------------------------- |
| `@tauri-apps/api`     | MIT OR Apache-2.0 | Typed Tauri IPC (`invoke`, `Channel`, `listen`) |
| `diff` (`jsdiff`)     | BSD-3-Clause      | Word-level diff rendering in popup and History  |
| `react` / `react-dom` | MIT               | User interface rendering                        |

## Development & Tooling Dependencies (`package.json`)

| Package                                             | License           | Purpose                                             |
| --------------------------------------------------- | ----------------- | --------------------------------------------------- |
| `@tailwindcss/vite` / `tailwindcss`                 | MIT               | Utility CSS & `@theme` design tokens                |
| `@tauri-apps/cli`                                   | MIT OR Apache-2.0 | Tauri development and bundling CLI                  |
| `@types/node` / `@types/react` / `@types/react-dom` | MIT               | TypeScript type declarations                        |
| `@vitejs/plugin-react` / `vite`                     | MIT               | Frontend dev server and bundler                     |
| `oxfmt`                                             | MIT               | Formatter (Prettier-compatible)                     |
| `oxlint`                                            | MIT               | Linter (including React Hooks rules)                |
| `typescript`                                        | Apache-2.0        | Static type checking (TypeScript 7 native compiler) |
| `vitest`                                            | MIT               | Pure-logic frontend unit test runner                |
