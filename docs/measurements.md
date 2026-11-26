# Redline Performance Measurements

Idle RAM includes `redline.exe` plus all of its `msedgewebview2.exe` child processes.
Budgets are set after Milestone 2 from measured baselines and checked in Milestone 5.

| Milestone    | Date       | Idle RAM (MB)         | Hotkey → Popup (ms) | Lint 12k chars (ms) | Release Binary / Install Size | Notes                                                                                       |
| ------------ | ---------- | --------------------- | ------------------- | ------------------- | ----------------------------- | ------------------------------------------------------------------------------------------- |
| 0 — Scaffold | 2026-11-26 | _pending measurement_ | N/A                 | N/A                 | _pending_                     | Tray-only baseline (before hidden popup webview in M2). CI green: FE ~20s, Rust 6m14s cold. |
