#![no_main]

libfuzzer_sys::fuzz_target!(|bytes: &[u8]| saaio::fuzzing::program_status(bytes));
