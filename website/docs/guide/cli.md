# CLI

```sh
taku init              # write a starter Takufile.lua
taku list              # tasks with their doc lines        (alias: ls)
taku run <task>        # run a task and its dependencies   (alias: r)
```

`Takufile.lua` is read from the current directory only. Running `taku` with no
command prints help.

## `taku run` flags

| Flag | Effect |
|---|---|
| `-n`, `--dry-run` | print the dependency tree and every step; execute nothing |
| `--vars KEY=VAL` | set a task parameter (repeatable; only names declared in the header) |
| `-y`, `--yes` | auto-answer `confirm` steps |
| `-f`, `--force` | run even when an `unchanged` guard says nothing changed |
| `--explain` | print why an `unchanged` guard skipped or rebuilt |
| `-j`, `--jobs N` | cap parallel tasks (default: CPU count) |

`--vars` applies to the requested task, not to its dependencies.

## Dry run

`--dry-run` prints the dependency tree, then each task's steps in execution
order:

```
dev
├─ build
└─ api

build:
  unchanged: the remaining steps would run
  cargo build --profile ${profile}
  <lua Takufile.lua:12>
api:
  serve "cargo run -p api" ready={http="http://127.0.0.1:8000/health";}
```

Command templates stay unresolved (secrets are not printed), function steps
show as `<lua file:line>`, and `unchanged` guards report what a real run would
skip. A dry run never touches the system.

## Exit status

`taku run` exits non-zero when any task fails: a command exits non-zero
(without `allow_fail`), a `confirm` is declined, a service dies, or a Lua
error is raised. The first failure stops scheduling new tasks.
