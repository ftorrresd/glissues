# glissues

`glissues` is a keyboard-first terminal issue list with live preview for managing GitLab issues as todo items.

It is built around the GitLab REST API and supports:

- creating issues as todos
- editing title and markdown body
- closing and reopening issues
- label editing with autocomplete
- comments
- blockers
- due date picking
- filters for state, label, and free-text search
- multiple stored projects with per-project theme memory
- async background preload for stored projects
- plain-text private token storage in the local config file

## Build

This project targets the system libc toolchain.

```bash
cargo build --release
```

The binary is written to `target/release/glissues`.

## Configuration

`glissues` reads configuration from these sources:

1. command-line flags
2. environment variables
3. `~/.config/glissues/config.toml`

CLI / environment settings for opening a new project:

- `project` as a full GitLab project URL
- `private_token`

Environment variables:

- `GLISSUES_PROJECT`
- `GLISSUES_PROJECT_URL`
- `GLISSUES_PRIVATE_TOKEN`

The config file stores:

- the last opened project
- the last selected theme
- stored projects
- GitLab private tokens in plain text

For a brand-new project, pass a project URL and private token through CLI flags or environment variables.
When the project is not already stored, `glissues` asks whether you want to save it.

Stored projects can later be opened without passing a private token again because the token is saved directly in `~/.config/glissues/config.toml`.

This is convenient, but it means your stored GitLab tokens are not encrypted at rest.

A sample config file is included as `config.example.toml`.

## Run

```bash
export GLISSUES_PROJECT="https://gitlab.cern.ch/ftorresd/todo"
export GLISSUES_PRIVATE_TOKEN="your-private-token"
cargo run --release
```

You can also pass the project URL directly:

```bash
cargo run --release -- --project "https://gitlab.cern.ch/ftorresd/todo" --private-token "your-private-token"
```

## Install

To install the latest released version into your user-local bin directory:

```bash
curl -fsSL https://raw.githubusercontent.com/ftorrresd/glissues/main/scripts/install.sh | sh
```

The installer downloads the newest GitHub release for your platform and installs `glissues` into `~/.local/bin` by default.

If `~/.local/bin` is not already on your `PATH`, add it to your shell profile:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

## Keybindings

- `j` / `k`: move through the issue list
- `gg` / `G`: jump to top or bottom of the list
- `Enter`: open the selected issue in a popup
- `Esc`: close the open issue popup or leave an overlay
- `j` / `k` or arrows in the issue popup: scroll the issue content
- `Ctrl-u` / `Ctrl-d`: scroll the open issue faster
- `Ctrl-r`: refresh from GitLab
- `p`: open the project picker
- `P`: cycle to the next known project
- `[` / `]`: cycle between known projects
- `t`: open the theme selector and cycle themes with `h`/`l` or arrows
- `n`: create a new issue
- `e`: edit selected issue
- `D`: delete the selected issue after confirmation
- `x`: close or reopen selected issue
- `c`: add a comment
- `b`: add a blocker
- `B`: remove a blocker
- `a`: edit labels with autocomplete
- `d`: open due date picker
- `Tab`: cycle all/open/closed filter
- `F`: filter by label
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

## Automation

- Pull requests and pushes to `main` run formatting, tests, and a release-build check in GitHub Actions
- Published GitHub releases build and upload release archives for supported Linux and macOS targets

## Notes

- GitLab issue `opened` / `closed` remains the source of truth for lifecycle state.
 - Startup preloads all known projects in the background, and each project preload includes issues, comments, and blocker links.
 - Active-project refreshes and edits run in the background so the TUI stays responsive during GitLab requests.
 - The UI uses `ratatui-themes` with Rosé Pine as the default theme, and your last chosen theme is remembered in `~/.config/glissues/config.toml`.
 - Stored project private tokens are saved in plain text in `~/.config/glissues/config.toml`.
 - New stored projects inherit the current theme, and each stored project remembers its own theme.
