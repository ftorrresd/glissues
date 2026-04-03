# AGENTS.md

This file is for coding agents working in `glissues`.
It captures the repository-specific commands, expectations, and coding style.

## Scope

- Applies to the whole repository.
- There is currently no existing `AGENTS.md` to preserve.
- There are no Cursor rules in `.cursor/rules/`.
- There is no `.cursorrules` file.
- There is no `.github/copilot-instructions.md` file.

## Project Summary

- `glissues` is a Rust TUI for managing GitLab issues as todo items.
- It uses the GitLab REST API with blocking `reqwest` calls.
- The main UI is a list + preview layout with popup overlays.
- Theme support is implemented through `ratatui-themes`.
- Startup and refresh preload comments and blocker links into memory.
- Config lives in `~/.config/glissues/config.toml` for optional local state like theme.

## Important Paths

- `src/main.rs` - terminal setup and event loop.
- `src/app.rs` - application state and input handling.
- `src/ui.rs` - ratatui rendering and popup widgets.
- `src/gitlab.rs` - GitLab HTTP client.
- `src/config.rs` - CLI/env/config loading and persistence.
- `src/model.rs` - API response structs.
- `src/editor.rs` - text buffer editing behavior.
- `src/markdown.rs` - lightweight markdown rendering.
- `scripts/install.sh` - latest-release installer.
- `.github/workflows/ci.yml` - CI checks.
- `.github/workflows/release.yml` - release build workflow.

## Build Commands

- Debug build: `cargo build`
- Release build: `cargo build --release`
- Locked release build: `cargo build --release --locked`
- Fast compile check: `cargo check`
- Locked compile check: `cargo check --locked`
- Format all Rust code: `cargo fmt`
- Verify formatting only: `cargo fmt --check`

## Test Commands

- Run all tests: `cargo test`
- Run all tests with locked dependencies: `cargo test --locked`
- Run all test targets: `cargo test --all-targets`
- Run a single test by exact name:
  - `cargo test config::tests::parses_project_url_into_base_and_path -- --exact`
  - `cargo test config::tests::saves_theme_into_config_file -- --exact`
  - `cargo test editor::tests::insert_str_appends_text_at_cursor -- --exact`
- Run tests matching a substring: `cargo test saves_theme`

## Lint / Verification Commands

- Formatting gate: `cargo fmt --check`
- Main local verification sequence:
  - `cargo fmt`
  - `cargo test`
  - `cargo check`
  - `cargo build --release`
- Optional clippy pass if needed: `cargo clippy --all-targets -- -D warnings`

## Run Commands

- Run from env:
  - `GLISSUES_PROJECT="https://gitlab.example.com/group/project" GLISSUES_PRIVATE_TOKEN="..." cargo run --release`
- Run from CLI flags:
  - `cargo run --release -- --project "https://gitlab.example.com/group/project" --private-token "..."`

## GitHub Actions Expectations

- CI should remain green on `cargo fmt --check`, `cargo test --locked --all-targets`, and `cargo build --release --locked`.
- Release workflow builds archives for Linux and macOS.
- If changing release packaging, also update `scripts/install.sh` and `README.md`.

## General Coding Style

- Use Rust 2024 edition conventions already present in the repo.
- Prefer small, focused functions over very long monolithic logic.
- Keep code ASCII unless the file already intentionally uses Unicode symbols.
- Match existing TUI structure instead of introducing a new architecture.
- Keep changes local to the relevant module when possible.
- Do not add large abstractions unless the code clearly benefits.

## Formatting Style

- Always run `cargo fmt` after Rust changes.
- Keep line breaks and wrapping consistent with rustfmt output.
- Avoid manual alignment that rustfmt will undo.

## Imports

- Group imports in this order:
  1. `std`
  2. external crates
  3. `crate::...`
- Keep imported symbols minimal.
- Remove unused imports promptly.
- Follow the existing pattern of explicit type imports rather than wildcard imports.

## Naming Conventions

- Types and enums: `PascalCase`
- Functions and methods: `snake_case`
- Constants: `SCREAMING_SNAKE_CASE`
- Fields and locals: `snake_case`
- Use descriptive names like `issue_view_scroll`, `selected_issue`, `blocker_picker`.

## Types and Data Modeling

- Prefer concrete structs for UI state rather than loose tuples.
- Use enums for modal state and action categories.
- Derive `Debug` and `Clone` where existing patterns expect it.
- Keep API models close to GitLab payloads and use `#[serde(default)]` where fields may be absent.
- Avoid premature generic abstractions.

## Error Handling

- Use `anyhow::Result` for fallible application logic.
- Add context to HTTP and file errors with `context(...)` / `with_context(...)`.
- Bubble errors up instead of swallowing them.
- UI-triggered errors should remain human-readable because they are shown in popup alerts.
- Preserve detailed HTTP context when possible, especially for GitLab API failures.

## Config Rules

- Project URL and private token are mandatory from CLI flags or environment.
- Do not reintroduce default project or private token values.
- The config file is for optional local state, not secrets required by default.
- Theme persistence should continue using `~/.config/glissues/config.toml`.

## TUI / UX Rules

- Preserve the current list + preview layout unless explicitly asked otherwise.
- Keep keyboard-first interactions as the default.
- Use popup overlays for secondary workflows.
- Keep editing direct-entry; do not reintroduce modal insert/normal editor behavior.
- Preserve cursor visibility and blinking behavior while text editing.
- Maintain draft persistence for issue edits and comment drafts.
- Keep mention picker behavior: typing `#` opens picker, `Enter` inserts `#iid`, `Esc` skips.

## Theme Rules

- Use `ratatui-themes` instead of ad hoc hardcoded full-theme systems.
- Theme cycling currently uses `Theme` / `ThemeName` and `ThemePicker`.
- Prefer semantic palette fields (`accent`, `fg`, `muted`, `warning`, `error`, etc.) over custom raw colors.
- If a new UI element needs color, derive it from the active theme palette first.

## GitLab API Rules

- Reuse `GitLabClient` for network operations.
- Keep blocking HTTP behavior unless a larger async refactor is explicitly requested.
- When adding API features, update both model structs and preload/cache behavior if detail views depend on them.
- For issue detail performance, prefer loading/caching at refresh time when feasible.

## Tests

- Add unit tests in the same source file under `#[cfg(test)]` when practical.
- Follow existing inline test style in `src/config.rs` and `src/editor.rs`.
- Add tests for parsing, config persistence, and pure editing logic first.
- Avoid brittle TUI snapshot tests unless necessary.

## Documentation Updates

- Update `README.md` when changing:
  - install flow
  - CLI/config requirements
  - keybindings
  - theme behavior
  - release behavior
- Keep examples copy-pasteable.

## Installer / Release Rules

- If changing release artifact names, update `.github/workflows/release.yml` and `scripts/install.sh` together.
- Installer must stay user-local only and should not require sudo.
- Prefer `~/.local/bin` as the default install target.

## Agent Workflow Tips

- Before finalizing, run at least `cargo fmt` and `cargo check`.
- For behavior changes, prefer `cargo test` too.
- If changing config or release behavior, also inspect `README.md`, workflows, and installer script.
- Do not remove user-facing features without an explicit request.
- Keep current dirty-worktree changes unless the user asks otherwise.
