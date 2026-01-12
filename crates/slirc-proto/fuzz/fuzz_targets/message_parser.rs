//! Fuzz target for IRC message parsing
//!
//! This fuzzer tests the robustness of the IRC message parser by feeding it
//! randomly generated input data and ensuring it doesn't panic or crash.
//! Tests both owned Message parsing and zero-copy MessageRef parsing.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::str;

fuzz_target!(|data: &[u8]| {
    // Only fuzz valid UTF-8 strings to focus on protocol-level issues
    if let Ok(input) = str::from_utf8(data) {
        // Skip empty inputs and very long inputs (over 8191 bytes is the IRC limit)
        if input.is_empty() || input.len() > 8191 {
            return;
        }
        
        // Test owned Message parsing - should never panic
        let _ = input.parse::<slirc_proto::Message>();
        
        // Test zero-copy MessageRef parsing - should never panic
        let _ = slirc_proto::MessageRef::parse(input);
        
        // Note: We don't compare results because Message and MessageRef have
        // different command name semantics (normalized vs raw). The important
        // thing is that neither parser panics.
        
        // Test IRC codec sanitization - should never panic
        let _ = slirc_proto::IrcCodec::sanitize(input.to_string());
    }
});