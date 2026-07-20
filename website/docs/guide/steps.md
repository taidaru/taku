# Steps

A step is **data**: a bare verb like `rm "path"` or `write {...}` does nothing
by itself — it returns a description that the runner executes in order. This
is what makes `--dry-run`, `unchanged`, and readable failure output possible.

All step constructors:

| Step | Does |
|---|---|
| `"cargo build"` | run a command |
| `argv { "prog", "arg", ... }` | run a command from explicit arguments |
| `pipe { "cmd1", "cmd2" }` | run a pipeline |
| `rm "path"` | delete a file or directory (recursive) |
| `mkdir "path"` | create a directory with parents |
| `cp { "src", to = "dst" }` | copy a file |
| `mv { "src", to = "dst" }` | move / rename |
| `write { "data", to = "file" }` | write a file |
| `append { "line", to = "file" }` | append a line (adds a newline) |
| `download { "url", to = "file", sha256 = "..." }` | fetch over HTTP(S) |
| `echo "text"` | print a line |
| `confirm "question?"` | ask `[y/N]`; cancelling fails the task |
| `invoke "task"` | run another task's steps inline |
| `unchanged { ... }` | skip the rest when inputs did not change — [Incremental](./incremental.md) |
| `serve { ... }` | start a long-lived service — [Services](./services.md) |
| `function(ctx) ... end` | arbitrary Lua — [Variables](./variables.md) |

Every string argument is a template: `${param}` and `$ENV_VAR` placeholders
are resolved just before the step runs (see [Variables](./variables.md)).

## Commands

A bare string runs a command. The string is tokenized the way a shell would
read it — quotes group words — but **no shell is involved**: nothing expands,
nothing is re-interpreted, interpolated values cannot inject extra commands.

```lua
task "build <profile=dev>" {
    "cargo build --profile ${profile}",
    "sh -c 'grep TODO src/*.rs | wc -l'",   -- want a shell? run one explicitly
}
```

A non-zero exit code fails the step and the task. The error message shows the
command **template**, not the resolved values — secrets pulled from the
environment never end up in the output.

Options go on the table form, with the command at position 1:

```lua
{ "make install", cwd = "sub", env = { CC = "clang" }, allow_fail = true, timeout = 60 }
```

| Option | Meaning |
|---|---|
| `cwd` | working directory |
| `env = {...}` | extra environment variables for this command |
| `allow_fail = true` | a non-zero exit does not fail the task |
| `timeout` | seconds; on expiry the process is killed and the step fails |
| `stdin` | string fed to the command's stdin |

Command processes inherit Taku's environment; a [`.env`](../api/env.md) file
fills in variables the environment leaves unset, and the step's `env =` wins
over both.

## argv and pipe

`argv` takes the argument list explicitly. Each element is formatted on its
own and never re-tokenized — use it when a value may contain spaces or quotes:

```lua
argv { "git", "commit", "-m", "${message}" }
```

`pipe` connects commands like `a | b | c`, managed by the runner. If any stage
fails, the step fails (pipefail — always):

```lua
pipe { "cat access.log", "grep ' 500 '", "wc -l" }
```

Both accept the same options as commands.

## Files

`rm` and `mkdir` take a path; `cp`, `mv`, `write`, `append` take a table with
the payload at position 1 and the target under `to`:

```lua
task "dist" {
    mkdir "dist",
    cp { "target/release/app", to = "dist/app-${version}" },
    write { "${version}", to = "dist/VERSION" },
    append { "built ${version}", to = "build.log" },
    rm "tmp",
}
```

## download

```lua
download {
    "https://example.com/tool.tar.gz",
    to = "vendor/tool.tar.gz",
    sha256 = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
}
```

With `sha256` set, a hash mismatch deletes the downloaded file and fails the
step.

## confirm

`confirm "question?"` prints the question and waits for `y`. Anything else
fails the task. Under `--yes`, or when there is no terminal to ask (CI), it
answers itself:

```lua
task "db-reset" {
    confirm "wipe the local database?",
    "dropdb app && createdb app",
}
```

## invoke

`invoke "name"` runs another task's steps right here, inside the current task
— unlike a dependency, which runs before the task starts. `invoke "name" { p = v }`
passes parameter values.

An invoke always executes. Its completion still counts for the run: if a later
task depends on the invoked one, that dependency is already satisfied.
