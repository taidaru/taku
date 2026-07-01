# API

Inside a Takefile these globals are available, alongside Lua's `string`, `table`,
`math`, `utf8`, and `coroutine` libraries:

| Global | Purpose |
|---|---|
| [`sh`](./sh.md) | run commands |
| [`fs`](./fs.md) | filesystem |
| [`net`](./net.md) | TCP / HTTP(S) |
| [`env`](./env.md) | environment variables |
| [`ssh`](./ssh.md) | run on / talk to a remote host |
| `task`, `import` | define tasks, include files |

Everything else (`io`, `os`, `package`, `debug`, ...) is disabled — see
[Sandbox](../sandbox.md).

Paths are strings; file contents and command output are **byte strings**
(binary-safe). On failure, these functions raise a Lua error with context, which
you can catch with `pcall`.
