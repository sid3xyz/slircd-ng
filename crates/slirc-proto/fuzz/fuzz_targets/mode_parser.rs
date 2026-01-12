//! Fuzz target for IRC mode string parsing
//!
//! This fuzzer tests the mode parser for robustness against malformed input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use std::str;
use slirc_proto::mode::{Mode, ChannelMode, UserMode, ModeType};

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = str::from_utf8(data) {
        // Skip very long inputs
        if input.len() > 256 {
            return;
        }
        
        // Test mode parsing from string pieces - should never panic
        let pieces: Vec<&str> = input.split_whitespace().collect();
        if !pieces.is_empty() {
            let _ = Mode::<ChannelMode>::as_channel_modes(&pieces);
            let _ = Mode::<UserMode>::as_user_modes(&pieces);
        }
        
        // Test individual mode character parsing - should never panic
        for ch in input.chars() {
            let _ = ChannelMode::from_char(ch);
            let _ = UserMode::from_char(ch);
        }
    }
});