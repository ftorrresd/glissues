# glissues

`glissues` is a Yazi-inspired terminal UI for managing GitLab issues as todo items.

It is built around the GitLab REST API and supports:

- creating issues as todos
- editing title and markdown body
- closing and reopening issues
- workflow status via `status::...` labels
- label editing with autocomplete
- comments
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

- `gitlab_url`
- `project`
- `token`
- `theme`
- `status_labels`

Environment variables:

- `GLISSUES_GITLAB_URL`
- `GLISSUES_PROJECT`
- `GLISSUES_TOKEN`
- `GLISSUES_THEME`
- `GLISSUES_STATUS_LABELS`

A sample config file is included as `config.example.toml`.

## Run

```bash
export GLISSUES_TOKEN="your-token"
cargo run --release
```

The default target project is `ftorresd/todo` on `https://gitlab.cern.ch`.

## Keybindings

- `j` / `k`: move in the issue list
- `gg` / `G`: jump to top or bottom
- `Enter`: open the selected issue in a popup
- `Esc`: close the open issue popup or leave an overlay
- `j` / `k` or arrows in the issue popup: scroll the issue content
- `Ctrl-u` / `Ctrl-d`: scroll the open issue faster
- `n`: create a new issue
- `e`: edit selected issue
- `x`: close or reopen selected issue
- `c`: add a comment
- `L`: edit labels with autocomplete
- `S`: set issue status label
- `d`: open due date picker
- `Tab`: cycle all/open/closed filter
- `l`: filter by label
- `s`: filter by status
- `/`: search
- `:`: command mode
- `?`: help
- `Ctrl-c`: quit

Inside the editor/comment popups:

- `i`: insert mode
- `Esc`: back to normal mode
- `Tab`: switch fields
- `Ctrl-s`: save

## Notes

- Workflow state is modeled with labels like `status::todo`, `status::doing`, `status::blocked`, and `status::done`.
- GitLab issue `opened` / `closed` remains the source of truth for lifecycle state.
- Available themes: `dracula`, `tokyo night`, `catppuccin`, `rose pine`, `vim classic`, `monokai`.
