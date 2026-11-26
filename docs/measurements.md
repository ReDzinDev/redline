# Redline Performance Measurements

Idle RAM includes `redline.exe` plus all of its `msedgewebview2.exe` child processes.  
Hotkey-to-popup latency (`hotkey_to_popup_ms`) and Harper lint times (`elapsed_ms`) are logged in Redline's log file (**Settings › About › Open log folder**).

## Target Budgets (Checked in Milestone 5)

| Metric                                        | Target Budget | Notes                                                                                  |
| --------------------------------------------- | ------------- | -------------------------------------------------------------------------------------- |
| Idle RAM (tray + hidden low-memory popup)     | < 60 MB total | Measured across `redline.exe` + child `msedgewebview2.exe` processes after idle settle |
| Hotkey → Popup visible (`hotkey_to_popup_ms`) | < 50 ms       | Popup is pre-created hidden at startup                                                 |
| Harper lint (12,000-character popup cap)      | < 200 ms      | Warm linter after first-use initialization                                             |
| Release binary size (`redline.exe`)           | < 25 MB       | LTO, `codegen-units = 1`, `strip = true`, bundled Harper curated dictionary            |

## Milestone Log

| Milestone              | Date                | Idle RAM (MB)       | Hotkey → Popup (ms) | Lint 12k chars (ms) | Release Binary Size | Notes                                                                                                                                                   |
| ---------------------- | ------------------- | ------------------- | ------------------- | ------------------- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 0 — Scaffold           | 2026-11-26          | 4 MB                | N/A                 | N/A                 | _N/A_               | Tray-only baseline before hidden popup webview. CI green: FE 27s, Rust 6m19s cold.                                                                      |
| 1 — Bare loop & AI     | 2026-11-26          | _N/A_               | _N/A_               | _N/A_               | _N/A_               | Live Check #7 passed on all 5 curated models (`finish_reason == "stop"`, `reasoning_tokens == 0`, `~$0.00006` total). CI green: FE 28s, Rust 7m32s.     |
| 2 — Inline popup       | 2026-11-26          | _pending local run_ | _pending local run_ | N/A                 | _N/A_               | Pre-created popup with WebView2 `MemoryUsageTargetLevel = LOW` while hidden & `AreBrowserAcceleratorKeysEnabled = false`. CI green: FE 30s, Rust 5m28s. |
| 3 — Harper grammar     | 2026-11-26          | _pending local run_ | _pending local run_ | _pending local run_ | _N/A_               | Lazy `FstDictionary::curated()` + `LintGroup::new_curated`; CPU-only ndarray (no `wgpu-hal`). CI green: FE 26s, Rust 7m22s.                             |
| 4 — Diff & Main window | 2026-11-26          | _pending local run_ | _pending local run_ | _pending local run_ | _N/A_               | Main window created on demand and destroyed on close to free its webview. CI green: FE 27s, Rust 8m35s.                                                 |
| 5 — Polish & v0.1.0    | _pending local run_ | _measure_           | _measure_           | _measure_           | _measure_           | Record after `pnpm tauri build` and full `docs/testing.md` pass.                                                                                        |
