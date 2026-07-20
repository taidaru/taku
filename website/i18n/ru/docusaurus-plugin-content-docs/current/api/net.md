# net — сеть

Сетевой доступ для `function(ctx)`-шагов. Про скачивание как шаг таска см.
шаг `download` в [Шагах](../guide/steps.md).

HTTP обслуживает [`ureq`](https://docs.rs/ureq): HTTP/1.1, редиректы, gzip,
TLS через rustls — системный OpenSSL не нужен.

```lua
function(ctx)
    local body = net.get("https://api.example.com/version")
    net.download("https://example.com/tool.tar.gz", "vendor/tool.tar.gz")
end
```

| Функция | Результат |
|---|---|
| `net.get(url)` | тело ответа (байты); `http://` и `https://` |
| `net.download(url, path [, sha256])` | записать тело в `path` потоково; сверить хэш, если задан |
| `net.tcp(host, port, data)` | сырой TCP: отправить `data`, вернуть весь ответ |

- Не-2xx HTTP-статус — ошибка.
- `get` буферизует в памяти с потолком 64 МиБ; `download` пишет на диск, до
  8 ГиБ. При несовпадении `sha256` файл удаляется и поднимается ошибка.
- Таймаут запросов — 30 секунд.
