//! Fuzz target for IRC prefix parsing
//!
//! This fuzzer tests the prefix parser for robustness against malformed input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::str;
use slirc_proto::prefix::{Prefix, PrefixRef};

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = str::from_utf8(data) {
        // Skip very long inputs
        if input.len() > 256 {
            return;
        }
        
        // Test prefix parsing - should never panic
        let _ = input.parse::<Prefix>();
        let _ = PrefixRef::parse(input);
        
        // Test prefix creation - should never panic
        let _ = Prefix::new_from_str(input);
    }
});