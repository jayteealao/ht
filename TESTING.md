# Testing Guide: ht Recording & Streaming

This document describes how to test the recording and streaming features of ht
against upstream asciinema specifications and implementations.

## Quick Start

```bash
# Run all tests
cargo test

# Run only golden tests
cargo test golden

# Run ALiS encoding tests
cargo test alis

# Run in release mode (faster)
cargo test --release
```

## Test Categories

### 1. Unit Tests

**Location**: `src/streaming/alis.rs`, `src/recording/asciicast_v3.rs`

**Purpose**: Test individual encoding functions against spec examples.

**Examples**:
- `test_leb128_encoding()` - Validates LEB128 against known values
- `test_color_parsing()` - RGB hex parsing
- `test_output_event_encoding()` - Byte-level ALiS encoding

### 2. Golden Tests

**Location**: `src/recording/asciicast_v3_golden_tests.rs`
**Fixtures**: `testdata/golden_minimal.cast`, `testdata/alis_init_example.bin`

**Purpose**: Validate our output format against authoritative examples.

**Critical Tests**:
- `test_exit_event_data_type()` - Verifies exit status is JSON integer (not string)
- `test_our_writer_no_spurious_init_output()` - Ensures we DON'T emit init seq as first event
- `test_interval_monotonicity()` - Validates timing correctness

### 3. Integration Tests

**Location**: `tests/` (TODO)

**Purpose**: End-to-end validation with real PTY sessions.

**Planned**:
- Record a deterministic command and compare .cast structure
- Stream to local ALiS endpoint and decode binary messages
- Compare against asciinema CLI output (if installed)

## Known Issues from Upstream Comparison

See `docs/UPSTREAM_COMPARISON.md` for full analysis.

### Critical Issues

#### Issue #1: Exit Event Data Type (NEEDS VERIFICATION)

**Status**: ⚠️ UNRESOLVED

Our code currently uses:
```rust
let status_str = status.to_string();
self.write_event(interval, "x", &status_str)?;
```

This produces: `[0.887, "x", "0"]` (string)

The spec says: "data is a numerical exit status"

Golden fixture has: `[0.887, "x", 0]` (integer)

**Test**: `test_exit_event_data_type()` currently expects integer.

**Action Required**: Verify with actual asciinema CLI output and fix if needed.

#### Issue #2: Spurious Init Output Event

**Status**: ❌ CONFIRMED BUG

Our code emits init `seq` as first output event:
```rust
Event::Init(time, cols, rows, _pid, seq, _text) => {
    self.write_header(cols, rows, time)?;
    self.write_event(0.0, "o", &seq)?;  // BUG!
}
```

This is WRONG. Initial terminal state should NOT be emitted.

**Test**: `test_our_writer_no_spurious_init_output()` validates this.

**Action Required**: Remove line 95-96 in `recording/asciicast_v3.rs`.

### Medium Priority Issues

#### Issue #3: Missing EOT Support

**Status**: ❌ NOT IMPLEMENTED

ALiS spec requires EOT (0x04) event for persistent connections.

**Action Required**: Implement in `streaming/alis.rs` and handle in event loop.

#### Issue #4: Timestamp Type

**Status**: ⚠️ MINOR INCONSISTENCY

We cast `f64` to `i64` for header timestamp. Should use actual Unix timestamp.

**Action Required**: Fix in `recording/asciicast_v3.rs:157`.

## Running Comparisons Locally

### Prerequisites

```bash
# Install asciinema CLI (optional but recommended)
curl -sSL https://asciinema.org/install | sh
```

### Compare Our Output vs Upstream

1. **Record with asciinema CLI**:
```bash
asciinema rec /tmp/upstream.cast -c "echo 'test' && exit 0"
```

2. **Record with ht**:
```bash
cargo build --release
./target/release/ht record --out /tmp/ht.cast bash -c "echo 'test' && exit 0"
```

3. **Compare structures**:
```bash
# Check header fields
head -1 /tmp/upstream.cast | jq .
head -1 /tmp/ht.cast | jq .

# Check exit event
tail -1 /tmp/upstream.cast | jq .
tail -1 /tmp/ht.cast | jq .
```

4. **Critical validation**:
```bash
# Verify exit event data type
tail -1 /tmp/upstream.cast | jq '.[2] | type'
# Should output: "number" (not "string")
```

### Validate ALiS Binary Encoding

1. **Decode golden fixture**:
```bash
# Our golden Init event should decode to:
# EventType=0x01, LastId=0, Time=0, Cols=80, Rows=24, Theme=None, Data="Hello!"

hexdump -C testdata/alis_init_example.bin
# Expected:
# 00000000  01 00 00 50 18 00 06 48  65 6c 6c 6f 21           |...P...Hello!|
```

2. **Compare with spec example**:
```
From docs.asciinema.org:
\x01 \x00 \x00 \x50 \x18 \x00 \x06 Hello!
```

Should match exactly!

## CI Integration (TODO)

Add to `.github/workflows/test.yml`:

```yaml
- name: Run golden tests
  run: cargo test golden -- --nocapture

- name: Validate test fixtures
  run: |
    # Ensure golden files exist
    test -f testdata/golden_minimal.cast
    test -f testdata/alis_init_example.bin

    # Validate golden .cast file is valid JSON lines
    jq -e . testdata/golden_minimal.cast > /dev/null

- name: Compare with asciinema CLI (if available)
  run: |
    if command -v asciinema &> /dev/null; then
      asciinema rec /tmp/upstream.cast -c "echo test"
      ./target/release/ht record --out /tmp/ht.cast bash -c "echo test"
      # Compare structures (implementation needed)
    fi
```

## Adding New Test Fixtures

### asciicast v3 Files

1. Create `.cast` file in `testdata/`
2. Validate structure:
```bash
# Header must be valid JSON
head -1 your_file.cast | jq -e .

# Events must be 3-element arrays
tail -n +2 your_file.cast | jq -e '.| length == 3'
```

3. Add provenance note to `testdata/PROVENANCE.md`

### ALiS Binary Files

1. Create `.bin` file using `printf`:
```bash
printf '\x01\x00\x00\x50\x18\x00\x06Hello!' > testdata/your_event.bin
```

2. Validate with `hexdump`:
```bash
hexdump -C testdata/your_event.bin
```

3. Add expected byte breakdown to `PROVENANCE.md`

## Debugging Test Failures

### "Exit event data type mismatch"

Check what asciinema CLI actually produces:
```bash
asciinema rec /tmp/test.cast -c "exit 42"
tail -1 /tmp/test.cast | jq '.[2]'
```

If it's a string, our code is correct. If it's a number, we need to fix.

### "Spurious init output event"

This is a known bug. The test SHOULD fail until we fix line 95-96.

To verify the fix works:
```bash
cargo test test_our_writer_no_spurious_init_output -- --nocapture
```

### "ALiS binary mismatch"

Compare byte-by-byte with spec examples:
```bash
# Our encoding
rust_bytes=$(cargo test test_init_event_encoding -- --nocapture 2>&1 | grep "encoded")

# Spec example
printf '\x01\x00\x00\x50\x18\x00\x06Hello!' | hexdump -C
```

## References

- [asciicast v3 Specification](https://docs.asciinema.org/manual/asciicast/v3/)
- [ALiS v1 Protocol](https://docs.asciinema.org/manual/server/streaming/)
- [Implementation Audit](docs/IMPLEMENTATION_AUDIT.md)
- [Upstream Comparison](docs/UPSTREAM_COMPARISON.md)
