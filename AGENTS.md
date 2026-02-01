# Repository Guidelines

## Project Structure & Module Organization
- `src/main.rs`: App entry; initializes logging, storage, clipboard monitor, GPUI window.
- `src/app.rs`: Application state management (entries, selection, sidebar state).
- `src/clipboard.rs`: Clipboard monitoring system; polls for changes and creates entries.
- `src/models.rs`: Data models (`ClipboardEntry`) with title generation logic.
- `src/storage/*`: SQLite layer, migrations, and DB operations; DB lives in `~/Library/Application Support/kopi/kopi{.dev}.db`.
- `src/ui/*`: Theme configuration and styling utilities.
- `src/icons.rs`: SVG icon path constants.
- `src/assets.rs`: Asset loading for GPUI.
- `src/utils.rs`: Shared utility functions.
- `assets/`: App icons and static assets.
- `script/`: Helper scripts (`dev`, `lint`, `bundle-mac`).

## Build, Test, and Development Commands
- Build (debug): `cargo build`
- Run locally: `cargo run`
- Dev loop: `script/dev` (requires `cargo-watch`) — rebuilds on change and runs.
- Lint: `script/lint` — runs `clippy --deny warnings` and `cargo machete`.
- Format: `cargo fmt --all`
- Bundle macOS app: `script/bundle-mac` (wraps `cargo bundle --format osx`).

## Coding Style & Naming Conventions
- Rust 2024 edition; prefer idiomatic Rust, small modules.
- Formatting: `cargo fmt --all` before commit.
- Linting: keep `clippy` clean; no warnings in CI/PRs.
- Naming: `snake_case` for functions/vars/modules, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for consts.
- Logs: use `log` crate (`info!/debug!/warn!`) for observability.

## Testing Guidelines
- Runner: `cargo test`.
- Tests exist for `storage` module; add unit tests for pure logic in `clipboard` and `models` when contributing.
- Conventions: co-locate tests in `mod tests { ... }` within the same file; name with intent (e.g., `generates_title_from_content`).

## Commit & Pull Request Guidelines
- Commits: use imperative mood, concise summary (≤72 chars), e.g., `add inline title editing`.
- Group related changes; keep diffs focused.
- PRs: include purpose, screenshots for UI, steps to verify; link issues.
- Run `script/lint` and `cargo fmt --all` before opening a PR.

## Security & Configuration Tips
- Database: user data stored locally under Application Support; avoid checking in DB files.
- Secrets: none expected; do not hardcode identifiers beyond bundle ID.

## Dependency Research Workflow
- Prefer local sources first:
  - Inspect versions in `Cargo.lock` and graph via `cargo tree -e features`.
  - Browse sources in `~/.cargo/registry/src/*/<crate>-<version>/` or git checkouts in `~/.cargo/git/checkouts/`.
  - Explore quickly with `rg` (e.g., `rg --line-number "pub struct" ~/.cargo/registry/src --crate <crate>`).
  - Generate local docs when available: `cargo doc -p <crate> --open`.
- If missing locally, then web search:
  - Prefer `docs.rs/<crate>`, `crates.io/crates/<crate>`, and the linked repository/CHANGELOG.
  - Cross-check the exact version from `Cargo.lock` to avoid API drift.
