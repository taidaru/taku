# sh — commands

Commands are **argument lists** run directly, with no shell. Behaviour doesn't
depend on `bash`/`cmd`, and interpolated values can't be re-parsed (no injection).

```lua
sh.run({ "cargo", "build" })             -- streams stdio, returns exit code

local msg = 'release: "v1.0" (final)'
sh.run({ "git", "commit", "-m", msg })   -- msg needs no quoting or escaping

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
sh.run({ "sh", "-c", "echo hello | tr a-z A-Z | tee log" })
```

## Options

The same `opts` apply to both `sh.run` and `sh.capture`:

```lua
fs.mkdir("build")
sh.run({ "sh", "-c", "echo $CC && pwd && cat" }, {
    cwd = "build",              -- working directory
    env = { CC = "clang" },     -- extra environment variables (added to the inherited env)
    stdin = "data on stdin",    -- fed to the command's stdin
})
```
