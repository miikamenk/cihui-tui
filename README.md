# Cihui 词汇 TUI

---

**Chinese Vocabulary Tool**

A ratatui-based terminal application for learning Chinese words and their pronunciation. Paste Chinese text or images to get pinyin and English translations.

## Translation

The app uses free translation APIs:

- translate.google.com
- MyMemory API
- LibreTranslate
- LibreTranslate/LTEngine\* (local)

auto mode goes from top of this list to the bottom in case of failure, you can choose your wanted translation engine in config

\* you need to manually install LTEngine and add it to path or set the binary location in `.config/cihui-tui/config.json`

## Building

**building text/OCR bin**

```bash
cargo build --release --bin cihui-ocr --features ocr --no-default-features
```

**building transcription bin**

```bash
cargo build --release --bin cihui-transcribe --features transcription,vulkan --no-default-features # you can use cuda instead of vulkan
```

The binary will be at `target/release/cihui-tui`.

![TUI image](/img/tui.png)
