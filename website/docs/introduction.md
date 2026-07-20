---
slug: /
---

# Introduction

Taku is a task runner. You describe project tasks — build, test, deploy — in a
`Takufile.lua`, and Taku executes them with their dependencies, in parallel
where possible.

The core idea: **a task is a plan, not a script**. A task body is a list of
steps that Lua builds when the file loads and the Rust core executes. Because
the plan is data, Taku can print it (`--dry-run`), skip it when inputs did not
change (`unchanged`), and explain its decisions (`--explain`) — things a shell
script cannot do.

```lua
--- generate sources
task "gen" {
    write { "1.0.0", to = "version.txt" },
}

--- build the project
task "build <profile=dev>: gen" {
    unchanged { "src/**/*.rs", outputs = "target" },
    "cargo build --profile ${profile}",
}
```

```sh
taku run build                          # gen, then build
taku run build                          # skips: nothing changed
taku run build --vars profile=release   # parameter override
taku run build --dry-run                # print the plan, touch nothing
```

What you get:

- **One file, one language.** Tasks are Lua — loops, conditionals, and string
  manipulation instead of Make syntax or YAML.
- **Cross-platform.** Commands run without a shell; the same Takufile works on
  Linux, macOS, and Windows.
- **Sandboxed.** Lua's `io`/`os` are disabled. A Takufile touches the system
  only through Taku's steps and modules, and only while a task runs — loading
  a Takufile never has side effects. See [Sandbox](./sandbox.md).
- **Incremental.** The `unchanged` guard fingerprints inputs and skips work.
- **Services.** `serve` starts dev servers and databases, waits until they are
  ready, and shuts them down when the run ends.

Start with [Getting started](./getting-started.md), then read the
[Guide](./guide/tasks.md).
