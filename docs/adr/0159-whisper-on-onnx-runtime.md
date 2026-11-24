# ADR 0159 — Whisper on ONNX Runtime, whisper.cpp removed

## Status

Accepted · Issue: #1304 · Spec 0108

## Context

After Parakeet (ADR 0157) the speech crate had two native stacks: whisper.cpp (CMake build, GGML models) and ONNX Runtime. The issue leaves open whether Whisper uses an engine layout the Parakeet code already supports or an own decoder on `ort`.

## Decisions

| Question | Decision | Why |
|---|---|---|
| Model layout | Hugging Face `onnx-community/whisper-base` / `-small`: `encoder_model_int8.onnx`, `decoder_model_merged_int8.onnx` (key/value cache), `vocab.json`, `added_tokens.json`, `config.json`, pinned to one commit per repo | The Parakeet engine is a transducer pipeline and cannot run an encoder/decoder (attention) model, so there is nothing to reuse for decoding; the Optimum export is the one with a maintained cache-enabled decoder |
| Decoder | Own greedy decoder on `ort` (`crates/speech/src/whisper.rs`): language detection from the logits after `<|startoftranscript|>`, prompt `[sot, lang, transcribe, notimestamps]`, token masks from `config.json`, no-speech check with the `<|nocaptions|>` probability, loop guard | No timestamps and no beam search are needed for dictation; keeps the engine small |
| Features | Own log-mel front end (`whisper/mel.rs`), same constants as the reference implementation, direct DFT instead of an FFT crate; frames fully behind the audio are not computed | No new dependency; the cost stays below the encoder's |
| Language setting | Same accepted list as before (ISO codes and English names); a name now maps to its code | `parse_language` no longer asks whisper.cpp |
| Model directory | Each Whisper model gets its own directory (`whisper-base-int8`, `whisper-small-int8`), like Parakeet; the engine is picked by the files in the directory | Single-file special-casing in the model manager is gone |
| Old files | `ggml-base.bin`, `ggml-small.bin` and `.part` leftovers in `speech-models/` are deleted at start (local only) | Frees the disk space; the models show `not_downloaded` and ask for consent again |
| Build settings | `GGML_NATIVE` removed; `MACOSX_DEPLOYMENT_TARGET` kept (other native build scripts use it, it matches `minimumSystemVersion`); 16 KB page-size flag kept | Only whisper.cpp needed the first |
