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

## Compliance Status

See `docs/UPSTREAM_COMPARISON.md` and `docs/VALIDATION_SUMMARY.md` for full analysis.

**Current Status**: ✅ **100% COMPLIANT** (22/22 features passing)

All critical and medium priority issues have been resolved:

### Issue #1: Exit Event Data Type ✅ FIXED

**Status**: ✅ RESOLVED (commit 979b2ee)

**What was wrong**: Exit event used string instead of integer
```rust
// Before (WRONG):
let status_str = status.to_string();
self.write_event(interval, "x", &status_str)?;  // → [0.887, "x", "0"]
```

**Fix**: Added `write_event_with_number()` method
```rust
// After (CORRECT):
self.write_event_with_number(interval, "x", status)?;  // → [0.887, "x", 0]
```

**Test**: `test_exit_event_data_type()` - **PASS** ✅

### Issue #2: Spurious Init Output Event ✅ FIXED

**Status**: ✅ RESOLVED (commit 979b2ee)

**What was wrong**: Emitted terminal dump as first output event
```rust
// Before (WRONG):
Event::Init(time, cols, rows, _pid, seq, _text) => {
    self.write_header(cols, rows, time)?;
    self.write_event(0.0, "o", &seq)?;  // BUG - removed
}
```

**Fix**: Removed spurious write_event() call
```rust
// After (CORRECT):
Event::Init(time, cols, rows, _pid, _seq, _text) => {
    self.write_header(cols, rows, time)?;
    // Do NOT emit init seq - recording starts from first real output
}
```

**Test**: `test_our_writer_no_spurious_init_output()` - **PASS** ✅

### Issue #3: EOT Support ✅ IMPLEMENTED

**Status**: ✅ RESOLVED (commit e33e4d2)

**What was added**: Complete ALiS v1 EOT (End of Transmission) support

**Implementation**:
- Added `EventType::EOT = 0x04` to event enum
- Implemented `encode_eot(id, rel_time)` function
- Format: EventType + LastId + RelTime (no data payload)

**Purpose**: Signals stream end without closing WebSocket connection

**Test**: `test_eot_event_encoding()` - **PASS** ✅

### Issue #4: Timestamp Type ✅ FIXED

**Status**: ✅ RESOLVED (commit e33e4d2)

**What was wrong**: Header timestamp used event time cast to i64

**Fix**: Use actual Unix timestamp
```rust
// Before (WRONG):
header["timestamp"] = json!(timestamp as i64);

// After (CORRECT):
let timestamp = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap()
    .as_secs();
header["timestamp"] = json!(timestamp);
```

**Location**: `src/recording/asciicast_v3.rs:154-159`

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

All known issues have been fixed. If you encounter test failures:

### General Debugging Steps

1. **Run specific test with output**:
```bash
cargo test test_name -- --nocapture
```

2. **Check test against spec**:
- Review `docs/UPSTREAM_COMPARISON.md` for spec references
- Compare against examples at https://docs.asciinema.org

3. **Verify test fixtures are intact**:
```bash
# Validate golden .cast file
jq -e . testdata/golden_minimal.cast

# Check binary fixtures
hexdump -C testdata/alis_init_example.bin
```

### "Exit event data type mismatch"

**This should not occur** - fixed in commit 979b2ee.

If it does, verify the fix is present at `src/recording/asciicast_v3.rs:117-119`:
```rust
Event::Exit(_time, status) => {
    let interval = self.calculate_interval();
    self.write_event_with_number(interval, "x", status)?;  // Must use write_event_with_number
}
```

### "Spurious init output event"

**This should not occur** - fixed in commit 979b2ee.

If it does, verify lines 93-94 in `src/recording/asciicast_v3.rs` are NOT present:
```rust
// These lines should NOT exist:
// self.write_event(0.0, "o", &seq)?;
```

The Init handler should only call `write_header()`, not emit any events.

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
