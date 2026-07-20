# Tasks

A task is a header string plus a list of steps:

```lua
--- build the project
task "build <profile=dev>: gen" {
    unchanged { "src/**/*.rs", outputs = "target" },
    "cargo build --profile ${profile}",
}
```

Lua builds the step list when the Takufile loads; the runner executes it when
you call `taku run build`. Nothing in a task body runs at load time.

## The header

```
"name <param> <param=default>: dep1 dep2"
```

| Part | Meaning |
|---|---|
| `name` | the task name, used in `taku run <name>` and in dependency lists |
| `<param>` | a parameter without a default — must be set with `--vars param=...` |
| `<param=default>` | a parameter with a default value |
| `: dep1 dep2` | dependencies, run before this task |

Parameters become `${param}` placeholders in the task's steps (see
[Variables](./variables.md)). Only declared names are accepted by `--vars`;
a typo gets a did-you-mean hint.

## Doc comments

A block of `---` lines directly above `task` documents it. `taku list` shows
the first line:

```lua
--- deploy to production
--- requires TOKEN in the environment
task "deploy <env=staging>" {
    ...
}
```

## Dependencies

`taku run <task>` first runs the transitive dependencies. Rules:

- Each task runs **at most once per invocation**, no matter how many tasks
  depend on it.
- Independent tasks run **in parallel**, up to `-j` (default: CPU count).
- Unknown dependencies and cycles are reported before anything runs.
- The first failure stops scheduling; tasks already running finish.

An **aggregator** is a task with an empty step list — a named group of deps:

```lua
task "ci: test lint build" {}
```

## Two call forms

The curried form `task "header" { steps }` is canonical. The parenthesized
form `task(header, steps)` does the same and is needed when the header is an
expression:

```lua
for _, name in ipairs({ "core", "ui", "api" }) do
    task("check-" .. name, {
        "cargo check -p " .. name,
    })
end
```

Defining the same name twice prints a warning; the last definition wins.

## Splitting a Takufile

`import` loads another `.lua` file into the same task set:

```lua
-- Takufile.lua
import("tasks/build.lua")
```

- Paths are relative to the file that calls `import` (nested imports resolve
  against their own file).
- The file runs in the same [sandbox](../sandbox.md) with the same globals.
- Each file is imported at most once, so two files can share a common helper.

## Isolation

Every task executes in its own fresh Lua state: there is no shared Lua state
between tasks, and top-level Takufile code runs once per executed task. Keep
top-level code to task definitions.
