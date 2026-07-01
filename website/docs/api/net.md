# net — network

HTTP is handled by [`ureq`](https://docs.rs/ureq) (HTTP/1.1, redirects, gzip, and
TLS via rustls — no system OpenSSL).

```lua
local body = net.http_get("https://example.com/")
net.download("https://example.com/file.tar.gz", "file.tar.gz")
```

| Function | Result |
|---|---|
| `net.http_get(url)` | response body (bytes); `http://` and `https://` |
| `net.download(url, path)` | fetch `url`, write the body to `path` |
| `net.tcp_request(host, port, data)` | raw TCP: send `data`, read the full response |

A non-2xx HTTP status raises an error. `http_get` caps the body at ~10 MB;
`download` allows large files.
