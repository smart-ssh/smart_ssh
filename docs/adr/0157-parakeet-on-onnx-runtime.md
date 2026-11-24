# ADR 0157 — Parakeet TDT 0.6B v3 on ONNX Runtime

## Status

Accepted · Issue: #1298 · Spec 0108

## Context

Voice input gets a third local model, Parakeet TDT 0.6B v3 (int8 ONNX export). The issue leaves open the engine crate and how ONNX Runtime is linked.

## Decisions

1. **Own engine on `ort`, not `parakeet-rs`.** The pinned export ships a mel preprocessor (`nemo128.onnx`), encoder, decoder/joint and `vocab.txt`. A small own implementation (preprocessor, encoder, greedy TDT decoding with the duration head) uses exactly these four verified files, needs no tokenizer or FFT crates, and is covered by pure unit tests for the decoder. It was run against the real model on German and English audio.
2. **Static linking via `ort`'s `download-binaries`.** The build script of `ort-sys` fetches the prebuilt static library once at build time and checks its hash; the binary needs no extra file next to it and nothing is fetched at run time. `ort` only compiles in with `default-features = false`; telemetry is switched off explicitly. The dependency is target-gated off Android.
3. **Catalog of files.** `ModelSource` is a list of `ModelFile`s (URL, size, SHA-256). All files go to `<name>.part` and are verified one after the other; only when every file matched are they renamed. A failure or cancel removes all temporary files, so no partial model exists.
4. **Engine choice by path.** A model path that is a directory is a Parakeet model, a file a Whisper model (`SpeechTranscriber`). The recorder and traits stay unchanged.
5. **Long audio** is cut into pieces of at most 30 s at the quietest point near the limit, because attention cost grows with length.
6. **`config.json` is not downloaded**; the feature size (128) and subsampling are fixed by the pinned model.

## Consequences

- Binary size grows by ONNX Runtime; the CI of every desktop target must build it.
- Windows and macOS builds of the static library are verified by CI only.
