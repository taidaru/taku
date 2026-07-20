# Getting started

Install Taku ([all options](installation.md)), then create a starter file in
your project root:

```sh
taku init
```

`taku init` writes a `Takufile.lua` with commented examples. A minimal one
looks like this:

```lua
--- run the test suite
task "test" {
    "cargo test",
}

--- format and lint
task "lint" {
    "cargo fmt --check",
    "cargo clippy",
}

--- everything CI runs
task "ci: test lint" {}
```

Three tasks: `test` and `lint` each run commands, `ci` is an aggregator — no
steps of its own, it only pulls in dependencies (the names after `:` in the
header).

```sh
taku list        # every task with its doc line
taku run ci      # test and lint, in parallel
taku run lint    # just lint
```

Useful from day one:

```sh
taku run ci --dry-run    # show what would run, run nothing
taku run ci -j 1         # sequential output
```

The full command and flag reference is in [CLI](./guide/cli.md). How tasks,
steps, and placeholders work is in the [Guide](./guide/tasks.md).

Notes:

- `Takufile.lua` is read from the current directory only; Taku does not search
  parent directories.
- A `.env` file next to the Takufile is loaded automatically — see
  [env](./api/env.md).
