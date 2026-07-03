# net — network

HTTP is handled by [`ureq`](https://docs.rs/ureq) (HTTP/1.1, redirects, gzip, and
TLS via rustls — no system OpenSSL).

```lua
local body = net.http_get("https://example.com/")
net.download(
    "https://github.com/taidaru/taku/releases/download/v0.1.2-alpha/taku-x86_64-unknown-linux-gnu.tar.gz",
    "taku.tar.gz"
)
```

| Function | Result |
|---|---|
| `net.http_get(url)` | response body (bytes); `http://` and `https://` |
| `net.download(url, path)` | fetch `url`, write the body to `path` |
| `net.tcp_request(host, port, data)` | raw TCP: send `data`, read the full response |

A non-2xx HTTP status raises an error. `http_get` buffers the body in memory and
caps it at 64 MiB; `download` streams to disk and allows up to 8 GiB. Requests
time out after 30 seconds.
