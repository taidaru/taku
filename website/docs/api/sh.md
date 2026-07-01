# sh — commands

Commands are **argument lists** run directly, with no shell. Behaviour doesn't
depend on `bash`/`cmd`, and interpolated values can't be re-parsed (no injection).

```lua
sh.run({ "cargo", "build" })             -- streams stdio, returns exit code
sh.run({ "git", "commit", "-m", msg })   -- msg needs no quoting
local r = sh.capture({ "git", "rev-parse", "HEAD" })
print(r.code, r.stdout, r.stderr)
```

- `sh.run(argv [, opts])` → exit code. Inherits stdio (live output).
- `sh.capture(argv [, opts])` → `{ code, stdout, stderr }`. `stdout`/`stderr` are
  byte strings.

A non-zero exit is **returned**, not raised — check `code` yourself. Only a
failure to launch the command (e.g. the program is not on `PATH`) raises a Lua
error.

A bare string is rejected. For shell features (pipes, globs, redirects), run a
shell yourself:

```lua
sh.run({ "sh", "-c", "make && ./run | tee log" })
```

## Options

The same `opts` apply to both `sh.run` and `sh.capture`:

```lua
sh.run({ "make" }, {
    cwd = "build",              -- working directory
    env = { CC = "clang" },     -- extra environment variables (added to the inherited env)
    stdin = "data on stdin",    -- fed to the command's stdin
})
```
