# net — network

Network access for `function(ctx)` steps. For downloads as task steps see the
`download` step in [Steps](../guide/steps.md).

HTTP is handled by [`ureq`](https://docs.rs/ureq): HTTP/1.1, redirects, gzip,
TLS via rustls — no system OpenSSL required.

```lua
function(ctx)
    local body = net.get("https://api.example.com/version")
    net.download("https://example.com/tool.tar.gz", "vendor/tool.tar.gz")
end
```

| Function | Result |
|---|---|
| `net.get(url)` | response body (bytes); `http://` and `https://` |
| `net.download(url, path [, sha256])` | stream the body to `path`; verify the hash if given |
| `net.tcp(host, port, data)` | raw TCP: send `data`, return the full response |

- A non-2xx HTTP status raises an error.
- `get` buffers in memory, capped at 64 MiB; `download` streams to disk, up to
  8 GiB. On a `sha256` mismatch the file is deleted and an error is raised.
- Requests time out after 30 seconds.
