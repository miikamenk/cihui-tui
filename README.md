# Cihui 词汇 TUI

---

**Chinese Vocabulary Tool**

A ratatui-based terminal application for learning Chinese words and their pronunciation. Paste Chinese text or images to get pinyin and English translations.

## Translation

The app uses free translation APIs:

- translate.google.com (primary)
- MyMemory API (fallback)
- LibreTranslate (fallback)

_might add baidu translate later_

Offline translation is not yet supported, I'm consider adding a local translation models.

## Building

```bash
cargo build --release
```

The binary will be at `target/release/cihui-tui`.

