//! Fuzz target for CTCP message parsing
//!
//! This fuzzer tests the CTCP parser for robustness against malformed input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::str;
use slirc_proto::ctcp::Ctcp;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = str::from_utf8(data) {
        // Skip very long inputs to focus on parsing logic
        if input.len() > 512 {
            return;
        }
        
        // Test CTCP parsing - should never panic
        let _ = Ctcp::parse(input);
        
        // Test is_ctcp check - should never panic
        let _ = Ctcp::is_ctcp(input);
    }
});