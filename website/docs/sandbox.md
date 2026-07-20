# Sandbox

A Takufile runs in a restricted Lua state. Only these standard libraries are
loaded: `coroutine`, `table`, `string`, `math`, `utf8`.

**Not available:** `io`, `os`, `package`, `debug`, `ffi`, and
`dofile`/`loadfile`. So a Takufile **cannot** touch the filesystem, run
processes, or load native code except through the Rust `cmd`/`fs`/`net`/`env`
APIs and the step executor.

On top of that, effects are gated by **phase**: while the Takufile loads, only
task definitions and reads are allowed; commands, writes, and network calls
work only while a task is actually running. Loading a Takufile — including
`taku list` and `--dry-run` — never touches the system.

`import` is the one controlled way to load more code: it reads only `.lua`
text and runs it in the same sandbox, so imported code can't escape it either.
