# Sandbox

A Takufile runs in a restricted Lua state. Only these standard libraries are
loaded: `coroutine`, `table`, `string`, `math`, `utf8`.

**Not available:** `io`, `os`, `package`, `debug`, `ffi`, and `dofile`/`loadfile`.
So a Takufile **cannot** touch the filesystem, run processes, or load native code
except through the Rust `fs`/`sh`/`net`/`env`/`ssh` APIs.

`import` is the one controlled way to load more code: it reads only `.lua` text
and runs it in the same sandbox, so imported code can't escape it either.
