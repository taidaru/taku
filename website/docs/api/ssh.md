# ssh — remote

A convenience runner over the OpenSSH `ssh` client, so keys, ssh-agent,
`~/.ssh/config`, and `known_hosts` work exactly as on the command line.

## One-off commands

```lua
ssh.run("deploy@host", { "uptime" })             -- exit code
local r = ssh.capture("deploy@host", { "hostname" })
```

These mirror [`sh.run`/`sh.capture`](sh) — same argument list and the same
`{ code, stdout, stderr }` result from `capture`, but executed on the remote host.
(The one-off forms take no `opts` table; inside `ssh.on` the rerouted `sh`
accepts the usual `cwd`/`env`/`stdin` options.)

## A whole block on the remote

Inside `ssh.on`, the ordinary `sh`/`fs`/`net`/`env` globals act on the **remote**
host for the duration of the block, then are restored:

```lua
local body = "[app]\nname = \"demo\"\n"

ssh.on({ host = "host", user = "deploy" }, function()
    fs.write("/srv/app/config.toml", body)               -- remote file
    sh.run({ "systemctl", "--user", "restart", "app" })  -- remote command
    local v = net.http_get("https://internal/health")    -- fetched FROM the host
end)
```

## Target

A string `"[user@]host[:port]"`, or a table:

```lua
{ host = "h", user = "u", port = 22, key = "~/.ssh/id_ed25519",
  password = "...", options = { "StrictHostKeyChecking=yes" } }
```

Authentication uses your keys/agent by default. With `password`, it is handed to
`ssh` via `SSH_ASKPASS` (never on the command line); the `known_hosts`
confirmation prompt still goes to your terminal as usual. In a headless run
(no terminal) that first-contact prompt cannot be answered — pre-populate
`known_hosts`, or pass e.g. `options = { "StrictHostKeyChecking=accept-new" }`.

Consecutive operations on the same host share one connection (OpenSSH
multiplexing, kept alive for 60 s; not available on Windows).

:::note
Remote `fs`/`env` run over coreutils (`cat`, `test`, `printenv`, ...); remote
`net` tunnels through `ssh -W`. No `curl`/`nc` is needed on the remote.
:::

:::note
Your local [`.env`](env#env-autoloading) still applies inside `ssh.on`: when a
variable is not set in the remote environment, `env.get` falls back to the
project's `.env`. The remote's real environment wins, and the fallback is
resolved locally — nothing from `.env` is sent to the host.
:::
