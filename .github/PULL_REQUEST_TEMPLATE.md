## Summary

Briefly describe what this PR changes and why.

## Checks

- [ ] `pnpm check` passes (`oxfmt --check`, `oxlint`, TypeScript 7 `tsc --noEmit`)
- [ ] `pnpm test` and `pnpm build` pass
- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml --check` passes
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --locked --all-targets -- -D warnings` passes
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml --locked` passes
- [ ] Tested manually against the relevant apps in [docs/testing.md](docs/testing.md)
- [ ] Confirmed no user text, prompts, AI results, or API keys are logged
- [ ] UI changes use design tokens in `src/styles/tokens.css` and user-facing copy in `src/lib/messages.ts`
