# Fuzzing slirc-proto

This directory contains fuzz targets for testing the robustness of the slirc-proto library using [cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz).

## Prerequisites

Install cargo-fuzz:

```bash
cargo install cargo-fuzz
```

## Running Fuzz Tests

### Message Parser Fuzzing

Test the main IRC message parser:

```bash
cargo fuzz run message_parser
```

### CTCP Parser Fuzzing

Test the CTCP message parser:

```bash
cargo fuzz run ctcp_parser
```

### Prefix Parser Fuzzing

Test IRC prefix parsing:

```bash
cargo fuzz run prefix_parser
```

### Mode Parser Fuzzing

Test IRC mode string parsing:

```bash
cargo fuzz run mode_parser
```

## Fuzzing Options

### Time-Limited Fuzzing

Run fuzzing for a specific duration:

```bash
cargo fuzz run message_parser -- -max_total_time=60
```

### Parallel Fuzzing

Run multiple fuzzing workers in parallel:

```bash
cargo fuzz run message_parser -- -workers=4
```

### Using a Custom Dictionary

Create input dictionaries for more targeted fuzzing:

```bash
# Create a dictionary with common IRC message patterns
echo -e "PRIVMSG\nPING\nJOIN\n:nick!user@host\n@time=2023-01-01T00:00:00Z" > irc.dict
cargo fuzz run message_parser -- -dict=irc.dict
```

## Analyzing Results

### View Crashes

If fuzzing finds crashes, they'll be saved in:

```
fuzz/artifacts/message_parser/
```

### Reproduce Crashes

To reproduce a specific crash:

```bash
cargo fuzz run message_parser fuzz/artifacts/message_parser/crash-<hash>
```

### Minimize Crashes

To find the minimal input that triggers a crash:

```bash
cargo fuzz tmin message_parser fuzz/artifacts/message_parser/crash-<hash>
```

## Coverage Analysis

Generate coverage reports to see what code paths are being tested:

```bash
cargo fuzz coverage message_parser
```

## Best Practices

1. **Run fuzzing regularly** - Integrate fuzzing into your CI/CD pipeline
2. **Start with short runs** - Begin with 5-10 minute runs to catch obvious issues
3. **Use dictionaries** - Create dictionaries with valid IRC protocol elements
4. **Monitor memory usage** - Fuzzing can be memory-intensive
5. **Test with different sanitizers** - Use AddressSanitizer and other sanitizers for different bug classes

## Integration with CI

Add fuzzing to your CI pipeline:

```yaml
# .github/workflows/fuzz.yml
name: Fuzz Testing
on:
  schedule:
    - cron: '0 2 * * *'  # Daily at 2 AM
  
jobs:
  fuzz:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: actions-rs/toolchain@v1
        with:
          toolchain: nightly
      - run: cargo install cargo-fuzz
      - run: cargo fuzz run message_parser -- -max_total_time=300
```

## Expected Results

The parsers should be robust against:

- Malformed UTF-8 sequences
- Extremely long lines
- Invalid IRC protocol sequences
- Missing or extra delimiters
- Control characters in unexpected positions
- Buffer overflows and underflows
- Integer overflows in length calculations

If fuzzing finds any crashes or hangs, please file an issue with the reproducing input.