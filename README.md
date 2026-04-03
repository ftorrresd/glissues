# glissues

`glissues` is a keyboard-first terminal issue list with live preview for managing GitLab issues as todo items.

It is built around the GitLab REST API and supports:

- creating issues as todos
- editing title and markdown body
- closing and reopening issues
- workflow status via `status::...` labels
- label editing with autocomplete
- comments
- blockers
- due date picking
- filters for state, label, status, and free-text search

## Build

This project targets the system libc toolchain.

```bash
cargo build --release
```

The binary is written to `target/release/glissues`.

## Configuration

`glissues` reads configuration from the first available source in this order:

1. command-line flags
2. environment variables
3. `~/.config/glissues/config.toml`

Supported settings:

- `project` as a full GitLab project URL
- `token`
- `status_labels`

Environment variables:

- `GLISSUES_PROJECT`
- `GLISSUES_PROJECT_URL`
- `GLISSUES_TOKEN`
- `GLISSUES_STATUS_LABELS`

Project URL and token are mandatory and must be provided through CLI flags or environment variables.
The config file is only used for optional local settings like `status_labels`.

A sample config file is included as `config.example.toml`.

## Run

```bash
export GLISSUES_PROJECT="https://gitlab.cern.ch/ftorresd/todo"
export GLISSUES_TOKEN="your-token"
cargo run --release
```

You can also pass the project URL directly:

```bash
cargo run --release -- --project "https://gitlab.cern.ch/ftorresd/todo" --token "your-token"
```

## Keybindings

- `j` / `k`: move through the issue list
- `gg` / `G`: jump to top or bottom of the list
- `Enter`: open the selected issue in a popup
- `Esc`: close the open issue popup or leave an overlay
- `j` / `k` or arrows in the issue popup: scroll the issue content
- `Ctrl-u` / `Ctrl-d`: scroll the open issue faster
- `Ctrl-r`: refresh from GitLab
- `t`: open the theme selector and cycle themes with `h`/`l` or arrows
- `n`: create a new issue
- `e`: edit selected issue
- `D`: delete the selected issue after confirmation
- `x`: close or reopen selected issue
- `c`: add a comment
- `b`: add a blocker
- `B`: remove a blocker
- `a`: edit labels with autocomplete
- `S`: set issue status label
- `d`: open due date picker
- `Tab`: cycle all/open/closed filter
- `F`: filter by label
- `s`: filter by status
- `/`: search
- `:`: command mode
- `?`: help
- `Ctrl-c`: quit

Inside the editor/comment popups:

- typing always inserts text
- `Esc`: close the current editor popup and keep the draft locally
- `Tab`: switch fields
- `#`: open issue mention picker and insert an issue reference like `#19`
- `Ctrl-s`: save

## Notes

- Workflow state is modeled with labels like `status::todo`, `status::doing`, `status::blocked`, and `status::done`.
- GitLab issue `opened` / `closed` remains the source of truth for lifecycle state.
- Startup and refresh preload issue comments into memory so opening issue details is immediate after loading completes.
- The UI uses `ratatui-themes` with Rosé Pine as the default theme, and you can cycle themes live from the built-in selector.
