# 🛡️ Nachtwache

> **Ultra-lekki, autonomiczny strażnik błędów i w 100% bezobsługowy drop-in zamiennik Sentry ze wsparciem AI.**  
> *Pojedyncza binarka, wbudowany dashboard, baza SQLite (WAL) i zużycie pamięci poniżej 20 MB RAM.*

[![License: MIT](https://img.shields.io/badge/License-MIT-amber.svg)](LICENSE)
[![RAM Footprint](https://img.shields.io/badge/RAM-~15MB-emerald.svg)]()
[![Binary Size](https://img.shields.io/badge/Binary-~18MB-blue.svg)]()
[![Sentry Protocol](https://img.shields.io/badge/Sentry%20SDK-100%25%20Compatible-orange.svg)]()

---

## ⚡ Dlaczego Nachtwache?

Oficjalne Sentry to wspaniały standard, ale:
1. **SaaS jest drogi**: darmowy limit 5k zdarzeń/mies. znika w godzinę przy jednym zapętlonym błędzie.
2. **Self-hosted to koszmar**: oficjalny Docker Compose to ponad **20 mikroserwisów** (Kafka, ClickHouse, Postgres, Redis, Snuba, Celery) i minimum **16–32 GB RAM**.
3. **Licencja BSL/FSL**: ogranicza swobodę komercyjną.

**Nachtwache rozwiązuje to od ręki:**
- **Zero konfiguracji**: Całość skompilowana do **1 pojedynczego pliku wykonywalnego** (serwer + wbudowany Web UI + baza SQLite WAL).
- **100% Drop-In Sentry SDK**: Nie zmieniasz ani jednej linijki kodu w aplikacjach poza adresem DSN.
- **AI Auto-Fix**: Klikasz jeden przycisk, a AI (lokalna Ollama, Gemini, OpenAI lub Claude) diagnozuje przyczynę błędu i generuje gotowy **git diff / patch**.
- **Prywatność i DSGVO**: Twoje dane nigdy nie opuszczają Twojego serwera.
- **Zasoby**: Zużywa **~15 MB RAM-u** i 0% CPU w spoczynku. Działa na najtańszym VPS za 4 EUR lub Oracle Cloud Free Tier.

---

## 🚀 Szybki Start

### 1. Uruchomienie binarki
```bash
# Sklonuj i zbuduj (wymaga Rust/Cargo):
git clone https://github.com/twoj-user/nachtwache.git
cd nachtwache
cargo build --release

# Uruchomienie serwera:
./target/release/nachtwache
```

Otwórz w przeglądarce: **`http://localhost:8080`**

Domyślny DSN do wklejenia w aplikacjach:
```
http://public@localhost:8080/1
```

---

## 🔌 Podłączenie aplikacji (Drop-In Sentry)

### JavaScript / TypeScript / React / Next.js
```typescript
import * as Sentry from "@sentry/browser";

Sentry.init({
  dsn: "http://public@localhost:8080/1", // lub https://sentry.social-wulf.eu/1
  tracesSampleRate: 1.0,
});
```

### Node.js / Express / NestJS
```typescript
import * as Sentry from "@sentry/node";

Sentry.init({
  dsn: "http://public@localhost:8080/1",
});
```

### Python / FastAPI / Django / Flask
```python
import sentry_sdk

sentry_sdk.init(
    dsn="http://public@localhost:8080/1",
    traces_sample_rate=1.0,
)
```

---

## 🤖 Konfiguracja AI (Auto-Fix)

W zakładce **Konfiguracja AI** w dashboardzie możesz wybrać preferowanego dostawcę:
- **Ollama (Darmowa, lokalna)**: `http://localhost:11434`, model `qwen2.5-coder:7b` (pełna prywatność).
- **Google Gemini**: klucz API, model `gemini-2.5-flash`.
- **OpenAI / DeepSeek / Groq**: klucz API, model `gpt-4o-mini` lub `deepseek-chat`.
- **Anthropic Claude**: klucz API, model `claude-3-5-sonnet-20241022`.

Gdy błąd trafi do systemu, kliknięcie **`[ ⚡ Diagnozuj z AI ]`**:
1. Odczytuje stack trace z podświetleniem linii `in_app`.
2. Analizuje ostatnie akcje użytkownika (*breadcrumbs*).
3. Wyjaśnia przyczynę awarii.
4. Generuje dokładny unified diff z poprawką.

---

## 🌐 Wdrożenie na VPS (Oracle Cloud / Hetzner + Caddy)

### 1. Systemd Service (`/etc/systemd/system/nachtwache.service`)
```ini
[Unit]
Description=Nachtwache AI Error Guardian
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/nachtwache
ExecStart=/opt/nachtwache/nachtwache
Restart=always
Environment=PORT=8080
Environment=HOST=127.0.0.1
Environment=NACHTWACHE_DB=/opt/nachtwache/nachtwache.db

[Install]
WantedBy=multi-user.target
```

### 2. Rewers Proxy z darmowym SSL (Caddyfile)
```caddy
sentry.social-wulf.eu {
    reverse_proxy 127.0.0.1:8080
}
```

---

## ⚖️ Prawne aspekty & Model „Co łaska”

- **Licencja MIT**: Pełne wyłączenie odpowiedzialności (*AS IS*).
- **Impressum & DSGVO**: Zgodne z § 5 DDG (niemieckie prawo telekomunikacyjne) i europejskim RODO – dane nie są przesyłane do USA ani sprzedawane.
- **Wsparcie**: Projekt tworzony z pasji w duchu *RobinHood AI* – jeśli oszczędza Ci setki dolarów miesięcznie, możesz dobrowolnie wesprzeć autora przez [BuyCoffee.to](https://buycoffee.to) lub GitHub Sponsors.

---

*Stworzone z pasją przez Adriana Wulfa.*
