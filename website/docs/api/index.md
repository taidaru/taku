# API modules

Modules perform effects **immediately**. They are for `function(ctx)` steps —
the escape hatch — while normal task bodies use [steps](../guide/steps.md),
which the runner executes itself.

| Module | Purpose |
|---|---|
| [`cmd`](./cmd.md) | run commands: `run`, `try`, `capture` |
| [`fs`](./fs.md) | filesystem: read, write, copy, glob, ... |
| [`net`](./net.md) | HTTP(S) requests, downloads, raw TCP |
| [`env`](./env.md) | environment variables and `.env` |

Shared rules:

- Effects are available **only while a task runs**. Calling them at the top
  level of a Takufile is an error; reads (`fs.read`, `fs.glob`, `env.get`, ...)
  work at load time too.
- Paths are strings. File contents and command output are **byte strings**,
  so binary data round-trips unchanged.
- Failures raise Lua errors with context; catch them with `pcall` if you need
  to.
- Modules do **not** apply `${...}` formatting to their arguments — use
  `fmt("...")` when you need placeholders (see
  [Variables](../guide/variables.md)).

Also global: `task`, `import`, `fmt`, `raw`, and every
[step constructor](../guide/steps.md). Lua's `string`, `table`, `math`,
`utf8`, and `coroutine` libraries are loaded; everything else is off — see
[Sandbox](../sandbox.md).
