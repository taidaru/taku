# net — сеть

HTTP реализован через [`ureq`](https://docs.rs/ureq) (HTTP/1.1, редиректы, gzip и
TLS через rustls — без системного OpenSSL).

```lua
local body = net.http_get("https://example.com/")
net.download("https://example.com/file.tar.gz", "file.tar.gz")
```

| Функция | Результат |
|---|---|
| `net.http_get(url)` | тело ответа (байты); `http://` и `https://` |
| `net.download(url, path)` | скачать `url`, записать тело в `path` |
| `net.tcp_request(host, port, data)` | сырой TCP: отправить `data`, прочитать весь ответ |

HTTP-статус не из диапазона 2xx бросает ошибку. `http_get` ограничивает тело
~10 МБ; `download` допускает большие файлы.
