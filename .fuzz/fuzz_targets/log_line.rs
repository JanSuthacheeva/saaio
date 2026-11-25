#![no_main]

libfuzzer_sys::fuzz_target!(|text: &str| saaio::fuzzing::log_line(text));
