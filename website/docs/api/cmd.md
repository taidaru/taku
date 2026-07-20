# cmd — commands

Runs a command **now**, inside a `function(ctx)` step. For commands as task
steps — bare strings, `argv`, `pipe` — see [Steps](../guide/steps.md).

```lua
function(ctx)
    cmd.run({ "cargo", "build" })                       -- raises on non-zero exit
    local code = cmd.try({ "git", "diff", "--quiet" })  -- returns the exit code
    local r = cmd.capture({ "git", "rev-parse", "HEAD" })
    print(r.code, r.stdout, r.stderr)
end
```

| Function | Behaviour |
|---|---|
| `cmd.run(argv [, opts])` | streams stdio; non-zero exit raises an error |
| `cmd.try(argv [, opts])` | streams stdio; returns the exit code |
| `cmd.capture(argv [, opts])` | returns `{ code, stdout, stderr }` |

- The command is an argv table: `{ "prog", "arg", ... }`. A bare string is
  rejected — there is no shell to parse it. For shell features run one
  explicitly: `{ "sh", "-c", "..." }`.
- `opts`: `cwd`, `env = {...}`, `stdin`, `timeout` (seconds; expiry kills the
  process and raises).
- `stdout`/`stderr` from `capture` are byte strings.
