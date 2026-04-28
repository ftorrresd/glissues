# glissues

`glissues` is a terminal app for working with GitLab issues as a personal todo list.

It gives you a keyboard-first issue list with a live preview pane, so you can quickly review, create, edit, comment on, close, reopen, label, and search issues without leaving your terminal.

## What you can do

- Browse GitLab issues in a fast terminal UI
- Create issues as todo items
- Edit issue titles and markdown descriptions
- Add comments
- Close and reopen issues
- Manage labels with autocomplete
- Set due dates
- Add and remove blockers
- Filter by state, label, or search text
- Switch between stored GitLab projects
- Choose and remember themes per project

## Install

### Install the latest release

```bash
curl -fsSL https://raw.githubusercontent.com/ftorrresd/glissues/main/scripts/install.sh | sh
```

The installer downloads the latest release for your platform and installs `glissues` into:

```text
~/.local/bin
```

If that directory is not on your `PATH`, add it to your shell profile:

```bash
export PATH="$HOME/.local/bin:$PATH"
```

Then start the app with:

```bash
glissues --project "https://gitlab.example.com/group/project" --private-token "your-token"
```

### Build from source

You can also build the binary locally with Cargo:

```bash
cargo build --release
```

The binary will be available at:

```text
target/release/glissues
```

## First run

To open a project, pass the full GitLab project URL and a GitLab private token:

```bash
glissues \
  --project "https://gitlab.example.com/group/project" \
  --private-token "your-token"
```

You can also use environment variables:

```bash
export GLISSUES_PROJECT="https://gitlab.example.com/group/project"
export GLISSUES_PRIVATE_TOKEN="your-token"
glissues
```

On first use, `glissues` can save the project so you do not need to pass the token every time.

## Command line options

```text
glissues [OPTIONS]
```

Options:

- `--project <URL>`: full GitLab project URL, for example `https://gitlab.com/group/project`
- `--private-token <TOKEN>`: GitLab private token used to access the project
- `--config <PATH>`: path to a custom config file
- `--help`: show command help
- `--version`: show the installed version

Supported environment variables:

- `GLISSUES_PROJECT`
- `GLISSUES_PROJECT_URL`
- `GLISSUES_PRIVATE_TOKEN`

Command line options take priority over environment variables and saved config.

## Configuration

By default, `glissues` stores its configuration at:

```text
~/.config/glissues/config.toml
```

The config file is used to remember:

- the last opened project
- stored projects
- the last selected theme
- per-project themes
- GitLab private tokens for saved projects

Stored GitLab private tokens are saved in plain text in the config file. Only save projects on machines where that is acceptable for you.

A minimal stored-project config looks like this:

```toml
last_project = "https://gitlab.example.com/group/project"
last_theme = "rose-pine"

[[projects]]
url = "https://gitlab.example.com/group/project"
private_token = "your-token"
theme = "rose-pine"
```

You can keep multiple projects in the same config file and switch between them inside the app.

## Basic controls

- `j` / `k`: move through issues
- `Enter`: open the selected issue
- `n`: create an issue
- `e`: edit the selected issue
- `c`: add a comment
- `x`: close or reopen the selected issue
- `a`: edit labels
- `d`: set due date
- `/`: search
- `F`: filter by label
- `Tab`: cycle issue state filter
- `p`: open project picker
- `t`: open theme selector
- `Ctrl-r`: refresh
- `?`: show help
- `Ctrl-c`: quit
