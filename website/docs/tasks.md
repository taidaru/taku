# Tasks

A task has a name and a body. There are two forms:

```lua
-- plain function: no dependencies
task("hello", function()
    print("hi")
end)

-- spec table
task("build", {
    desc = "compile the project",   -- optional; shown by `taku list`
    deps = { "hello" },             -- optional; run first
    run = function()                -- required, even with `deps`
        print("building")
    end,
})
```

The `run` function is **required** in the spec-table form — a spec table without
one is an error. Defining the same task name twice prints a warning and the last
definition wins.

## Dependencies

`taku run <task>` runs the transitive dependencies first. Each task runs **exactly
once per invocation**, even when several tasks list it as a dependency — a shared
dependency is never repeated. Unknown tasks and dependency cycles are reported as
errors before anything runs.

```lua
task("gen",   { run = function() print("gen") end })
task("build", { deps = { "gen" }, run = function() print("build") end })
task("test",  { deps = { "gen" }, run = function() print("test") end })
task("ci",    { deps = { "build", "test" }, run = function() print("ci") end })
```

`taku run ci` runs `gen` first (only once, although both `build` and `test`
depend on it), then `build` and `test` in parallel, then `ci`.

## Parallelism

Independent tasks run concurrently on separate threads, up to your CPU count by
default; limit it with `taku run <task> -j N`. The first failure stops scheduling
new tasks; work already in flight finishes.

Each task body runs in its **own fresh sandbox** — there is no shared mutable Lua
state between tasks, and top-level Takufile code runs once per executed task. Keep
top-level code to task definitions.

## Imports

Split a large Takufile across several files with `import`:

```lua
-- Takufile.lua
import("tasks/build.lua")
```

```lua
-- tasks/build.lua
task("build", function()
    print("building")
end)
```

- **Relative paths.** A path resolves against the directory of the file that calls
  `import`, not the current working directory. Nested imports resolve against their
  own file, so `tasks/build.lua` can `import("helpers.lua")` to reach
  `tasks/helpers.lua`.
- **`.lua` only, same sandbox.** `import` reads a `.lua` text file and runs it in
  the same restricted state — imported code gets the same `fs`/`sh`/... globals and
  the same [limits](./sandbox.md), so it can't escape the sandbox. A missing file
  raises an error.
- **Runs at load, once.** The file executes immediately at the point of `import`;
  its top-level `task(...)` calls register into the shared task set. Each file is
  imported **at most once** — importing it again, even through a different path to
  the same file, is a no-op. So two files can safely `import` a common helper.
- **Side effects only.** `import` returns nothing; use it to register tasks or
  define shared globals, not to return a value.
