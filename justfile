# Task runner for cihui-tui. Run `just` to list recipes.
#
# The suite is split by Cargo feature because the optional features pull in
# heavy native builds: `transcription` compiles whisper.cpp, and `ocr` builds
# the MNN C++ library through a git dependency. The default recipe needs
# neither, so it runs in seconds.

default:
    @just --list

# Fast hermetic suite: no native dependencies, no network.
test:
    cargo nextest run --no-default-features

# Suite plus the transcription-only build (compiles whisper.cpp).
test-transcribe:
    cargo nextest run --no-default-features --features transcription

# Suite plus the OCR build (compiles MNN through the ocr-rs git dependency).
test-ocr:
    cargo nextest run --no-default-features --features ocr

# Every feature combination the binaries ship as.
test-all: test test-transcribe test-ocr

# Tests that call the real translation APIs. Needs network, and will fail
# when a free service is down or rate limiting.
test-live:
    cargo nextest run --no-default-features --profile live \
        --run-ignored all -E 'binary(live_endpoints)'

# List the tests without running them.
list:
    cargo nextest list --no-default-features

# Review pending UI snapshot changes interactively.
snapshot:
    cargo insta test --no-default-features --review --test-runner nextest

# Regenerate the transcription-mode snapshots.
snapshot-transcribe:
    cargo insta test --no-default-features --features transcription --review \
        --test-runner nextest

# What CI runs on the fast path.
ci:
    cargo fmt --check
    cargo clippy --no-default-features --all-targets -- -D warnings
    cargo nextest run --no-default-features --profile ci

fmt:
    cargo fmt
