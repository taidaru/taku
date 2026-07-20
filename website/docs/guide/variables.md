# Variables and placeholders

Every string in a step is a template. The runner resolves it just before the
step executes.

| Syntax | Source | If missing |
|---|---|---|
| `${name}` | task variables | error |
| `$NAME`, `${$NAME}` | environment, then [`.env`](../api/env.md) | error |
| `$$` | a literal `$` | — |

A stray `$` that matches neither form is an error — templates never fail
silently.

**Task variables** come from three places, later ones win:

1. defaults in the header — `<profile=dev>`;
2. the command line — `--vars profile=release`;
3. assignments made by an earlier function step — `ctx.vars.profile = "..."`.

`--vars` accepts only names declared in the header. A typo is an error with a
did-you-mean hint.

**Environment variables are separate on purpose.** `$NAME` is only ever
resolved from the environment — a task variable can never satisfy it. Keep
secrets in the environment: when a command fails, Taku prints its unresolved
template, so resolved secret values never appear in output or logs.

`raw("...")` wraps a value the runner must pass through without formatting —
use it for data that itself contains `$`:

```lua
write { raw "cost: $5", to = "note.txt" }
```

## Function steps

A Lua function in the step list is the escape hatch for logic data steps
cannot express. It receives a context:

```lua
task "release <tag>" {
    function(ctx)
        local r = cmd.capture({ "git", "rev-parse", "--short", "HEAD" })
        ctx.vars.sha = r.stdout:gsub("%s+$", "")
    end,
    "git tag -a ${tag} -m 'release ${tag} (${sha})'",
}
```

- `ctx.vars` — a live table of the task's variables. Values you set feed the
  placeholders of every later step.
- `ctx.task` — the task's spec: name, params, deps, doc.
- `fmt("...")` — formats a template with the current variables, for use inside
  the function.

Inside a function step the [module APIs](../api/index.md) — `cmd`, `fs`,
`net`, `env` — perform effects directly.

## Load time vs run time

A Takufile is **loaded** (to know the tasks) and later its tasks are **run**.
Effects — commands, file writes, network — are only allowed while a task runs.
Calling `cmd.run` at the top level of a Takufile is an error; reads like
`fs.read`, `fs.glob`, and `env.get` work in both phases. That is why
`taku list` and `--dry-run` are guaranteed not to touch the system.
