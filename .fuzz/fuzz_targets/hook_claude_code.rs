#![no_main]

libfuzzer_sys::fuzz_target!(|text: &str| saaio::fuzzing::claude_code_hook(text));
