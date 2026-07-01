---
slug: /
---

# Introduction

Taku automates repetitive development tasks — build, test, deploy, lint, and the
like. Tasks are written in **Lua** in a `Takufile.lua` at your project root.

- A **Rust core** exposes a safe, cross-platform API (filesystem, processes,
  network, SSH) to Lua.
- **Lua** is the user-facing layer: you compose those APIs into tasks.

A Takefile runs in a [sandbox](./sandbox.md): Lua's own `io`/`os` are disabled, so
the only way to touch the system is through the provided `fs`/`sh`/`net`/`env`/
`ssh` globals. Tasks declare dependencies, and independent ones run in parallel.

```lua
task("build", {
    desc = "compile the project",
    deps = { "gen" },
    run = function()
        if sh.run({ "cargo", "build" }) ~= 0 then
            error("build failed")
        end
    end,
})
```

```sh
taku run build     # runs `gen`, then `build`
taku list          # shows all tasks
```
