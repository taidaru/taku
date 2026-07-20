# Examples

## A pipeline with dependencies

Dependencies run first, each at most once, so this builds in order:
`clean` → `gen` → `build`.

```lua
--- remove the build dir
task "clean" {
    rm "out",
}

--- generate sources
task "gen: clean" {
    mkdir "out",
    write { "1.0.0", to = "out/version.txt" },
}

--- assemble the artifact
task "build: gen" {
    cp { "out/version.txt", to = "out/app.txt" },
    echo "build: done",
}
```

```sh
taku run build       # clean -> gen -> build
```

## An incremental build with parameters

`unchanged` skips the expensive part when nothing changed; `--vars` overrides
a header parameter.

```lua
--- compile the project
task "build <profile=dev>" {
    unchanged { "src/**/*.rs", "Cargo.toml", outputs = "target" },
    "cargo build --profile ${profile}",
}
```

```sh
taku run build                        # full run
taku run build                        # skip (unchanged)
taku run build --explain              # says why
taku run build --vars profile=release # vars are part of the fingerprint
```

## A dev environment with services

`serve` keeps long-lived processes running; as deps they start in the
background and the graph continues once they're ready.

```lua
--- run database migrations
task "migrate" {
    "sqlx migrate run",
}

--- the API server
task "api: migrate" {
    serve {
        "cargo run -p api",
        ready = { http = "http://127.0.0.1:8000/health", timeout = 30 },
    },
}

--- the web frontend
task "web" {
    serve { "npm run dev", cwd = "frontend" },
}

--- the whole dev stack, until Ctrl+C
task "dev: api web" {}
```

```sh
taku run dev
```

## Mixing data steps with logic

A `function(ctx)` step computes values for later placeholders; `confirm`
guards a destructive run.

```lua
--- publish a release
task "release <tag>" {
    confirm "publish ${tag}?",
    function(ctx)
        local r = cmd.capture({ "git", "rev-parse", "--short", "HEAD" })
        ctx.vars.sha = r.stdout:gsub("%s+$", "")
    end,
    "git tag -a ${tag} -m 'release ${tag} (${sha})'",
    "git push origin ${tag}",
}
```

```sh
taku run release --vars tag=v1.2.0
taku run release --vars tag=v1.2.0 --dry-run   # preview: templates unresolved
```

## Generating tasks in a loop

Tasks are just Lua, so define them from data: one task per module, plus an
aggregate that depends on them all (they run in parallel).

```lua
local modules = { "core", "ui", "api" }

local names = {}
for _, name in ipairs(modules) do
    names[#names + 1] = "check-" .. name
    task("check-" .. name, {
        "cargo check -p " .. name,
    })
end

task("check-all: " .. table.concat(names, " "), {})
```

```sh
taku run check-all    # check-core, check-ui, check-api in parallel
```
