# env — environment

Read-only access to environment variables.

```lua
local mode = env.get("MODE", "dev")   -- value, or "dev" if unset
local token = env.require("TOKEN")    -- errors if unset
```

- `env.get(name [, default])` → the value, or `default` / `nil` if unset.
- `env.require(name)` → the value, or a Lua error if unset.

## `.env` autoloading

If a `.env` file sits next to your `Takufile.lua`, Taku loads it automatically and
uses its values as a **fallback**: a real environment variable always wins, and
`.env` only fills in what the environment does not already set.

```bash
# .env
API_URL=https://example.com
export TOKEN="abc 123"        # optional `export ` prefix; quotes are honored
SECRET='no $expansion here'   # single quotes are literal
APP_DIR=${BASE_DIR}/app       # ${VAR} substitution
DEBUG=1 # trailing comments after a value are stripped
```

Supported syntax: `KEY=VALUE` lines, `#` comment and blank lines, an optional
`export ` prefix, and single- or double-quoted values (inside double quotes the
`\n`, `\\`, and `\"` escapes are expanded). Values may reference other variables
with `${VAR}`: the real environment is consulted first, then variables defined
earlier in the same file; single-quoted values are taken literally. The file is
never written to and is only read on this machine.

Inside [`ssh.on`](ssh), the same local `.env` is also consulted as a fallback for
remote lookups — the remote host's real environment still takes precedence, and
the fallback is resolved locally, so nothing from your `.env` is sent to the host.
