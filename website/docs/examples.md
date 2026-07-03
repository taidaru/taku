# Examples

## A pipeline with dependencies

Dependencies run first, each at most once, so this builds in order:
`clean` → `gen` → `build`.

```lua
task("clean", {
    desc = "remove the build dir",
    run = function()
        if fs.exists("out") then fs.remove("out") end
    end,
})

task("gen", {
    desc = "generate sources",
    deps = { "clean" },
    run = function()
        fs.mkdir("out")
        fs.write("out/version.txt", "1.0.0\n")
        print("gen: wrote out/version.txt")
    end,
})

task("build", {
    desc = "assemble the artifact",
    deps = { "gen" },
    run = function()
        local version = fs.read("out/version.txt")
        fs.write("out/app.txt", "app " .. version)
        print("build: done")
    end,
})
```

```sh
taku run build       # clean -> gen -> build
```

## Generating tasks in a loop

Tasks are just Lua, so define them from data: one task per module, plus an
aggregate that depends on them all (they run in parallel).

```lua
local modules = { "core", "ui", "api" }

local stamps = {}
for _, name in ipairs(modules) do
    local task_name = "stamp-" .. name
    stamps[#stamps + 1] = task_name
    task(task_name, {
        desc = "stamp the " .. name .. " module",
        run = function()
            fs.mkdir("out")
            fs.write("out/" .. name .. ".stamp", "ok\n")
        end,
    })
end

task("stamp", {
    desc = "stamp every module",
    deps = stamps,        -- { "stamp-core", "stamp-ui", "stamp-api" }
    run = function() print("stamped " .. #stamps .. " modules") end,
})
```

## Fan-in: combine many outputs into one

Several tasks produce parts; a final task waits for them, then merges with `fs`.

```lua
local parts = { "header", "body", "footer" }

local part_tasks = {}
for i, part in ipairs(parts) do
    local name = "part-" .. part
    part_tasks[#part_tasks + 1] = name
    task(name, {
        run = function()
            fs.mkdir("parts")
            fs.write("parts/" .. i .. "-" .. part .. ".txt", part .. "\n")
        end,
    })
end

task("bundle", {
    desc = "concatenate the parts in order",
    deps = part_tasks,
    run = function()
        local names = fs.read_dir("parts")
        table.sort(names)
        local out = ""
        for _, name in ipairs(names) do
            out = out .. fs.read("parts/" .. name)
        end
        fs.write("bundle.txt", out)
        print("bundle: " .. #names .. " parts")
    end,
})
```

## Conditional steps with `env`

```lua
task("report", {
    run = function()
        local mode = env.get("MODE", "dev")     -- default when unset
        print("building in " .. mode .. " mode")
        if mode == "release" then
            fs.mkdir("out")
            fs.write("out/RELEASE", "")
        end
    end,
})
```

```sh
taku run report                 # building in dev mode
MODE=release taku run report    # ... and writes out/RELEASE
```

A `.env` next to your `Takufile.lua` is loaded automatically, so `MODE` could
instead live there (a real environment variable still overrides it):

```bash
# .env
MODE=release
```

See [`.env` autoloading](api/env#env-autoloading) for the full syntax.

## Calling your tools

Once you want to run your real build/test commands, use `sh`. A command is an
argument list (no shell); a tiny helper fails the task on a non-zero exit:

```lua
local function run(argv)
    local code = sh.run(argv)
    if code ~= 0 then
        error(table.concat(argv, " ") .. " failed (exit " .. code .. ")")
    end
end

-- Replace these with the commands your project actually uses.
task("test", { run = function() run({ "cargo", "test" }) end })
task("lint", { run = function() run({ "cargo", "clippy" }) end })

task("ci", {
    desc = "lint and test in parallel",
    deps = { "lint", "test" },
    run = function() print("ci: all green") end,
})
```

These need the named programs installed — see [sh](./api/sh.md) for capturing
output, `cwd`/`env`/`stdin` options, and shell pipelines.
