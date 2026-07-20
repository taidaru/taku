# Services

`serve` starts a long-lived process — a dev server, a database — that outlives
the step. It must be the **last** step of its task.

```lua
--- the API server
task "api: migrate" {
    serve {
        "cargo run -p api",
        ready = { http = "http://127.0.0.1:8000/health", timeout = 30 },
    },
}

--- the whole dev stack
task "dev: api web" {}
```

Forms:

```lua
serve "npm run dev"                          -- just a command
serve { "npm run dev", cwd = "frontend" }    -- with options (cwd, env, ready)
serve { api = { "..." }, web = { "..." } }   -- several services in one task
```

## Readiness

`ready` defines when the task counts as done and dependents may proceed:

| `ready` | Task completes when |
|---|---|
| *(absent)* | immediately after the process starts |
| `{ timeout = secs }` | after `secs` seconds |
| `{ http = "url", timeout = cap }` | the URL answers **2xx**; `timeout` caps the wait (default 30 s) |

The HTTP probe treats everything else — connection refused, a 5xx from a
warming-up server — as "not ready yet" and keeps polling. Only the timeout
expiring fails the task.

## Lifecycle

- As a **dependency**, a service starts in the background; once ready, the
  graph continues. When the run finishes, services are shut down.
- When **everything** you requested is services (or aggregators of services),
  Taku keeps them running: `taku run dev` prints
  `services running — press Ctrl+C to stop` and Ctrl+C stops the services with
  it.

Exit rules — a service is supposed to outlive the run, so exiting early means
something is wrong:

- exits before becoming ready → its task fails;
- exits with a non-zero code at any point → the run fails and **every other
  service is stopped**;
- exits cleanly while being held → it is removed from the hold; when none are
  left, the run ends normally.
