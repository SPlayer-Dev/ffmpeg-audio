---
"ffmpeg_audio": minor
---

**Added** `HttpProxy` and `HttpAudioSource::new_with_proxy` to choose the proxy policy of the HTTP client: the system/environment proxy (previous behavior, still the default), a direct connection, or an explicit `http://`, `https://` or `socks5://` proxy URL.
