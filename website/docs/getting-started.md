# Getting started

## Install

See [Installation](installation.md) for all options (install scripts, Homebrew,
Scoop, release archives, and building from source).

## Create a Takufile

```sh
taku init          # writes a starter Takufile.lua (won't overwrite an existing one)
```

## Commands

| Command | Alias | Description |
|---|---|---|
| `taku run <task>` | `r` | Run a task and its dependencies |
| `taku list` | `ls` | List defined tasks |
| `taku init` | | Create a starter `Takufile.lua` |

Running `taku` with no command prints help. `Takufile.lua` is read from the
current directory only (Taku does not search parent directories).

Independent tasks run in parallel up to your CPU count. Cap the number of
concurrent tasks with `-j` / `--jobs`:

```sh
taku run ci -j 2
```
