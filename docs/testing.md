# Redline Manual Testing Checklist

Run the **Core 5** apps for everyday changes (Milestones 1–4) and the **Full Matrix** before releases or whenever `src-tauri/src/platform/windows/` changes (Milestone 5).

## Preparation

1. Start Redline (`pnpm tauri dev` or the release binary).
2. Copy a **screenshot or image** (or a file in Explorer) to your clipboard before starting, so you can verify clipboard preservation.
3. Open **Win+V** (Windows Clipboard History) periodically to verify Redline never pollutes it.
4. Keep a sample English sentence with typos ready, e.g.: `Teh quick brown fox jump over the lazy dog.`

## App Compatibility Matrix

**Core 5 apps:** Notepad, Chrome, Slack, Word, VS Code.  
**Full Milestone 5 matrix:** all rows below.

| App                                | Capture path               | Replaced in place | `Ctrl+Z` undo in app | Copied image/files preserved | Win+V history clean | Notes                                                                          |
| ---------------------------------- | -------------------------- | ----------------- | -------------------- | ---------------------------- | ------------------- | ------------------------------------------------------------------------------ |
| **Notepad** _(Core)_               | UIA                        | [ ]               | [ ]                  | [ ]                          | [ ]                 | Also verify CRLF line endings preserved on multi-line paste                    |
| **Microsoft Word** _(Core)_        | UIA / Clipboard            | [ ]               | [ ]                  | [ ]                          | [ ]                 | Verify "No changes" closes without pasting plain text over formatted selection |
| **Outlook (classic)**              | UIA / Clipboard            | [ ]               | [ ]                  | [ ]                          | [ ]                 | Compose email body                                                             |
| **Outlook (new)**                  | UIA / Clipboard            | [ ]               | [ ]                  | [ ]                          | [ ]                 | WebView2 compose body                                                          |
| **Google Chrome** _(Core)_         | UIA / Clipboard            | [ ]               | [ ]                  | [ ]                          | [ ]                 | Test `<textarea>` and `contenteditable` (e.g. Gmail / Docs)                    |
| **Microsoft Edge**                 | UIA / Clipboard            | [ ]               | [ ]                  | [ ]                          | [ ]                 | Test `<textarea>` and `contenteditable`                                        |
| **Mozilla Firefox**                | UIA / Clipboard            | [ ]               | [ ]                  | [ ]                          | [ ]                 | Test `<textarea>` and `contenteditable`                                        |
| **Slack** _(Core)_                 | UIA / Clipboard            | [ ]               | [ ]                  | [ ]                          | [ ]                 | Message composer                                                               |
| **Discord**                        | UIA / Clipboard            | [ ]               | [ ]                  | [ ]                          | [ ]                 | Message composer                                                               |
| **Notion**                         | Clipboard                  | [ ]               | [ ]                  | [ ]                          | [ ]                 | Block editor                                                                   |
| **VS Code** _(Core)_               | Clipboard (UIA skip list)  | [ ]               | [ ]                  | [ ]                          | [ ]                 | Confirm `Code.exe` uses clipboard fallback without lag                         |
| **Windows Terminal**               | Clipboard (`Ctrl+Shift+C`) | [ ]               | N/A                  | [ ]                          | [ ]                 | Confirm selection captures via `Ctrl+Shift+C`                                  |
| **Notepad (Run as Administrator)** | Error path                 | N/A               | N/A                  | [ ]                          | [ ]                 | Popup shows admin-app message + **Open in editor**                             |

## Multi-Monitor & High-DPI Check

- [ ] Move target app to a **second monitor with a different DPI scaling factor** (e.g., 100% primary + 150% secondary).
- [ ] Trigger hotkey near all four screen corners: popup appears near cursor, flips inside the monitor's work area, and stays crisp and sized accurately when expanding from compact (`460×320`) to expanded (`460×520`).

## Offline Grammar (Harper) Checklist

Test **without** an OpenRouter API key saved:

- [ ] Select `Teh cat sat` → popup shows wavy red spelling underline on `Teh` and a suggestion card (`the`).
- [ ] Click **Apply** → span updates, card disappears, focus moves to the next card.
- [ ] Click **Fix all** (or press `F`) on a sentence with 2+ fixable errors → all fixes apply cleanly in one step; `Ctrl+Z` undoes the entire Fix all in one step.
- [ ] Click **Ignore** → card disappears for the rest of the popup session; returns on a fresh popup session.
- [ ] Press `Enter` after applying a Harper fix (with no AI action run) → pastes fixed text back into target app and records `Grammar fixes · Harper · $0` in History.
- [ ] Select non-English text (e.g. `Olá meu amigo, tudo bem?`) → popup shows `Grammar check: English only`.
- [ ] Change dialect in **Settings › Grammar** (e.g. American → British) → `colour` stops being flagged.

## Inline Popup & AI Streaming Checklist

Test with an OpenRouter API key saved in Settings:

- [ ] Press `1`–`4` → result streams live; footer shows `Action · Model · $cost · X.Xs`; pressing `D` toggles Result / Diff.
- [ ] Press `5` → tone chips appear (`1`–`4`); `Esc` closes tone chips without closing popup; second `Esc` closes popup and returns focus.
- [ ] Press `6` → custom prompt input focuses; submitting remembers instruction in Recent chips (up to 5).
- [ ] Verify all 6 rows of **What Enter does**:
  - [ ] Streaming → Enter does nothing.
  - [ ] Error or cut-off result → Enter does nothing; Retry / Copy work.
  - [ ] Complete AI result (changed) → Enter pastes result and records History entry.
  - [ ] Complete AI result (unchanged / "No changes") → Enter closes without pasting.
  - [ ] No AI result, Harper fixes applied → Enter pastes fixed text.
  - [ ] Nothing changed → Enter closes and returns focus.
- [ ] Pressing hotkey while popup is open toggles it closed and returns focus.
- [ ] Clicking outside the popup dismisses it cleanly (without firing during initial capture).
- [ ] Browser shortcuts (`F5`, `Ctrl+R`, `Ctrl+F`, `Ctrl+P`, `Ctrl++`) do nothing inside the popup.
- [ ] Selecting > 12,000 characters shows `Too long for the popup.` + **Open in editor**.
- [ ] Closing the target app before pressing `Enter` copies the result and shows `Copied: paste it with Ctrl+V.` instead of pasting blindly.

## Main Window, Tray & Settings Checklist

- [ ] **First launch:** Welcome screen appears once with hotkey summary, offline reassurance, optional key Save/Test, start-at-login toggle, and **Done**.
- [ ] **Write tab:** Typing debounces Harper lints (~300 ms); clicking a suggestion card selects the matching span in the textarea; `Ctrl+1`–`Ctrl+6` run AI actions; **Copy** and **Use this** work; > 40,000 chars disables AI actions with warning.
- [ ] **History tab:** Shows last 20 entries (both AI rewrites and Harper-only pastes); expanding shows word-level diff with **Copy original** and **Copy result**; **Clear history** empties list; turning history off shows notice.
- [ ] **Settings tab:** Press-to-record hotkey updates immediately (conflict rolls back and alerts); **Test** key shows label & limit remaining; curated model & custom model ID verifier work; privacy checkbox persists; **Open log folder** opens log directory.
- [ ] **Tray menu:** Left-click opens Write tab; **Pause hotkey** checks/unchecks and disables/re-enables the hotkey; **Settings** opens Settings tab; **Quit** exits cleanly.
- [ ] **Single instance:** Launching Redline a second time focuses the main window instead of spawning a second tray icon.
- [ ] **Accessibility:** Tab reaches every button, underlined word, card, tab, checkbox, and input with visible focus rings; Left/Right arrows move between Write / History / Settings tabs.
- [ ] **Log privacy check:** Open the log file via **Settings › About › Open log folder** and confirm zero user text, prompts, AI results, or API keys appear anywhere in the logs.
