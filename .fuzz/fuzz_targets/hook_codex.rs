#![no_main]

libfuzzer_sys::fuzz_target!(|text: &str| saaio::fuzzing::codex_hook(text));
