# Upstream Comparison: ht vs asciinema

**Date**: 2025-12-30
**Upstream Sources**:
- [asciicast v3 Specification](https://docs.asciinema.org/manual/asciicast/v3/)
- [ALiS v1 Protocol Specification](https://docs.asciinema.org/manual/server/streaming/)
- [asciinema CLI (GitHub)](https://github.com/asciinema/asciinema)
- [asciinema Server (GitHub)](https://github.com/asciinema/asciinema-server)
- [asciinema Player (GitHub)](https://github.com/asciinema/asciinema-player)

## Critical Findings

### 1. asciicast v3 Format Compliance

| Aspect | Upstream Spec | Our Implementation | Status |
|--------|---------------|-------------------|---------|
| **Header `version`** | Must be 3 (integer) | ✅ `json!(3)` | **PASS** |
| **Header `term`** | Required object with `cols`, `rows` | ✅ Present | **PASS** |
| **Header `term.type`** | Optional string | ✅ `--term-type` flag | **PASS** |
| **Header `term.theme`** | Optional object with `fg`, `bg`, `palette` | ✅ `--theme-fg/bg` flags | **PASS** |
| **Header `timestamp`** | Unix seconds (integer) | ❌ **MISMATCH**: We cast f64 to i64 | **MINOR** |
| **Header `env`** | Object (string → string) | ✅ Implemented | **PASS** |
| **Event format** | `[interval, code, data]` | ✅ Correct | **PASS** |
| **Event `o` (output)** | Data is raw terminal output string | ✅ Correct | **PASS** |
| **Event `r` (resize)** | Data is `"{COLS}x{ROWS}"` (string) | ✅ `format!("{cols}x{rows}")` | **PASS** |
| **Event `m` (marker)** | Data is label string | ✅ Correct | **PASS** |
| **Event `i` (input)** | Data is raw input string | ✅ Correct | **PASS** |
| **Event `x` (exit)** | Data is **numerical** exit status | ❌ **CRITICAL**: We use `.to_string()` | **FAIL** |
| **Event intervals** | Seconds since previous event (float) | ✅ `as_secs_f64()` | **PASS** |
| **Idle time limit** | Applied to intervals | ✅ `interval.min(limit)` | **PASS** |

#### Critical Issue #1: Exit Event Data Type

**Spec says**: "data is a numerical exit status of session's main child process"

**Our code** (line 108 in `recording/asciicast_v3.rs`):
```rust
Event::Exit(_time, status) => {
    let interval = self.calculate_interval();
    let status_str = status.to_string();  // ❌ WRONG - should be integer!
    self.write_event(interval, "x", &status_str)?;
}
```

**Upstream example** (from spec):
```json
[0.887, "x", "0"]
```

Wait, that IS a string in the JSON! Let me re-check...

**Re-analysis**: JSON arrays always serialize numbers as numbers. The example shows `"0"` which IS a JSON string. But logically, exit codes are integers. Need to verify if spec means JSON integer or JSON string.

Looking at the spec more carefully: "data is a numerical exit status" - this suggests it should be a number, not a string representation. But NDJSON serialization means it could be either.

**Decision**: ⚠️ UNCLEAR - Need to check actual .cast files from asciinema CLI

#### Issue #2: First Output Event

**Our code** (line 95 in `recording/asciicast_v3.rs`):
```rust
Event::Init(time, cols, rows, _pid, seq, _text) => {
    // ...
    self.write_header(cols, rows, time)?;

    // Write initial output as first event with 0 interval
    self.write_event(0.0, "o", &seq)?;  // ❌ Is this correct?
}
```

**Question**: Does asciinema CLI emit the initial terminal state (`seq` from `vt.dump()`) as a first output event?

**Spec**: Silent on this. The spec doesn't describe capturing or restoring initial terminal snapshot. Events are sequential from session start.

**Hypothesis**: The initial state is NOT emitted as an event. The terminal starts blank and only explicit output is recorded.

**Decision**: ❌ **PROBABLE BUG** - We should NOT emit init seq as first output event

### 2. ALiS v1 Binary Protocol Compliance

| Aspect | Upstream Spec | Our Implementation | Status |
|--------|---------------|-------------------|---------|
| **Magic string** | `[0x41, 0x4C, 0x69, 0x53, 0x01]` ("ALiS\x01") | ✅ `ALIS_MAGIC` constant | **PASS** |
| **LEB128 encoding** | Unsigned LEB128 for all integers | ✅ `encode_leb128()` with tests | **PASS** |
| **String encoding** | `[Length: LEB128][Data: UTF-8]` | ✅ `encode_string()` | **PASS** |
| **Init event** | `[0x01][LastId][Time][Cols][Rows][Theme][InitData]` | ✅ Correct order | **PASS** |
| **Init LastId** | Zero for new stream, non-zero for reconnect | ✅ We use 0 | **PASS** |
| **Init RelTime** | **Microseconds since stream start** | ✅ We use 0 | **PASS** |
| **Output event** | `[0x6F][Id][RelTime][Data]` | ✅ Correct | **PASS** |
| **Output RelTime** | **Microseconds since previous event** | ✅ `calculate_rel_time_micros()` | **PASS** |
| **Resize event** | `[0x72][Id][RelTime][Cols][Rows]` | ✅ Correct | **PASS** |
| **Marker event** | `[0x6D][Id][RelTime][Label]` | ✅ Correct | **PASS** |
| **Exit event** | `[0x78][Id][RelTime][Status: LEB128]` | ✅ Correct (integer) | **PASS** |
| **EOT event** | `[0x04][RelTime]` | ❌ **MISSING** | **FAIL** |
| **Theme format 0x00** | No theme data | ✅ Implemented | **PASS** |
| **Theme format 0x08** | `[Fg:RGB][Bg:RGB][8×RGB]` | ✅ Implemented | **PASS** |
| **Theme format 0x10** | `[Fg:RGB][Bg:RGB][16×RGB]` | ✅ Implemented | **PASS** |
| **RGB encoding** | `[R:u8][G:u8][B:u8]` | ✅ `parse_color()` | **PASS** |
| **WS subprotocol** | `v1.alis` for producers | ✅ `Sec-WebSocket-Protocol` header | **PASS** |
| **WS subprotocol** | `v3.asciicast` for v3 producers | ✅ Implemented | **PASS** |

#### Critical Issue #3: Missing EOT Support

**Spec says**: "This event may be used to signal the stream end without closing the connection."

**Purpose**: Allows persistent WebSocket connection that can receive multiple sessions (stream restart).

**Our implementation**: ❌ NOT IMPLEMENTED
- When PTY exits, we close WebSocket immediately
- No EOT event sent
- Consumer must reconnect for new session

**Impact**:
- Local `/ws/alis-v1` endpoint: Not critical (consumers can reconnect)
- Remote streaming: May cause issues with asciinema server expecting EOT

**Decision**: Should implement EOT for full compliance

### 3. WebSocket Subprotocol Negotiation

| Aspect | Upstream Spec | Our Implementation | Status |
|--------|---------------|-------------------|---------|
| **Producer header** | `Sec-WebSocket-Protocol: v1.alis` or `v3.asciicast` | ✅ Line 181-189 in `asciinema_server.rs` | **PASS** |
| **Consumer endpoint** | Only supports `v1.alis` | ✅ Line 73 in `api/http.rs` | **PASS** |
| **Fallback detection** | If no subprotocol header, detect from first message | ⚠️ Not applicable (we always set header) | **N/A** |

### 4. Timing Semantics

| Aspect | Upstream Spec | Our Implementation | Status |
|--------|---------------|-------------------|---------|
| **Clock source** | Monotonic recommended | ✅ `std::time::Instant` | **PASS** |
| **asciicast v3 intervals** | Seconds (float) since previous event | ✅ `duration_since().as_secs_f64()` | **PASS** |
| **ALiS RelTime** | **Microseconds** since previous event | ✅ `duration_since().as_micros()` | **PASS** |
| **ALiS Init Time** | **Microseconds since stream start** | ✅ We use 0 for new streams | **PASS** |
| **First event** | Interval = 0 | ✅ `last_event_time = None` → 0 | **PASS** |

### 5. InitData Semantics (ALiS)

**Spec says**: "Optional pre-existing terminal content, to bring the consumer up to speed with terminal state."

**Purpose**: Late-joining consumers get current terminal snapshot to sync state.

**Our implementation** (`streaming/alis_local.rs:136-142`):
```rust
Init(_time, cols, rows, _pid, seq, _text) => {
    // ...
    let bytes = alis::encode_init(0, 0, cols as u16, rows as u16, None, &seq)?;
    Ok(Some(ws::Message::Binary(bytes)))
}
```

**Analysis**: ✅ We pass `seq` from `vt.dump()` as InitData. This is correct!

### 6. Exit Event Encoding Mismatch

**asciicast v3**: Uses string (as JSON serialization)
**ALiS v1**: Uses LEB128 integer (binary encoding)

**Our code**:
- asciicast v3: `status.to_string()` ❌ Should be direct integer in JSON array
- ALiS v1: `encode_leb128(status as u64)` ✅ Correct

### 7. Example Byte Sequences (Validation)

#### From Upstream Spec

**Init with 80x24, no theme, "Hello!" data:**
```
\x01 \x00 \x00 \x50 \x18 \x00 \x06 Hello!
```

Breakdown:
- `\x01` → Event type (Init)
- `\x00` → LastId = 0 (LEB128)
- `\x00` → Time = 0 (LEB128)
- `\x50` → Cols = 80 (LEB128: 0x50 = 80)
- `\x18` → Rows = 24 (LEB128: 0x18 = 24)
- `\x00` → Theme format = none
- `\x06` → String length = 6 (LEB128)
- `Hello!` → 6 bytes of data

**Our encoding** (let's verify):
```rust
encode_init(0, 0, 80, 24, None, "Hello!")
```

Expected output:
- `encode_leb128(0)` = `[0x00]` ✅
- `encode_leb128(0)` = `[0x00]` ✅
- `encode_leb128(80)` = `[0x50]` ✅
- `encode_leb128(24)` = `[0x18]` ✅
- `encode_theme(None)` = `[0x00]` ✅
- `encode_string("Hello!")` = `[0x06, b'H', b'e', b'l', b'l', b'o', b'!']` ✅

**Result**: ✅ MATCHES SPEC

**Output "ls -la\n" after 125ms (125000μs) with Id=1:**
```
\x6F \x01 \xE8\x07 \x07 ls -la\n
```

Wait, the spec says "\xE8\x07" for 125000μs. Let's verify LEB128:
- 125000 = 0x1E8480
- LEB128 encoding: Should be multiple bytes...
- Actually 125000 in decimal, let me recalculate:
  - 125ms = 125,000μs = 0x1E848
  - LEB128: 0x1E848 = 125000
    - Low 7 bits: 72 (0x48) | 0x80 = 0xC8
    - Next 7 bits: 61 (0x3D) | 0x80 = 0xBD
    - Next 7 bits: 3 (0x03)
  - Result: [0xC8, 0xBD, 0x03]

Hmm, spec shows `\xE8\x07` which is only 2 bytes. Let me recalculate:
- If `\xE8\x07` is correct:
  - Byte 1: 0xE8 = 11101000 → value = 0x68, continue bit set
  - Byte 2: 0x07 = 00000111 → value = 0x07 << 7 = 0x380
  - Total = 0x68 + 0x380 = 0x3E8 = 1000 decimal

So "125ms" in the comment is wrong - it's actually 1000μs = 1ms!

**Re-checking our LEB128 implementation**:
- Our test: `encode_leb128(1000)` should give `[0xE8, 0x07]`
- Calculation: 1000 = 0x3E8
  - Low 7 bits: 0x68 | 0x80 = 0xE8 ✅
  - High bits: 0x07 (no continue bit) ✅
- Result: `[0xE8, 0x07]` ✅

Great, our LEB128 implementation is correct!

## Decision Matrix

| Issue | Severity | Action Required | Timeline |
|-------|----------|-----------------|----------|
| Exit event uses string in v3 | **HIGH** | Verify spec interpretation, check actual .cast files | **Immediate** |
| Missing EOT support | **MEDIUM** | Implement EOT event for ALiS | **Soon** |
| First output event (init seq) | **HIGH** | Remove spurious first output event | **Immediate** |
| Timestamp as f64→i64 | **LOW** | Use integer timestamps | **Soon** |

## Action Plan

### Priority 1: Critical Fixes

1. **Remove init seq as first output event** (Line 95-96 in `recording/asciicast_v3.rs`)
   - The initial terminal state should NOT be emitted as an output event
   - Recording should start with NO events until actual PTY output occurs

2. **Fix exit event data type in v3** (Line 108 in `recording/asciicast_v3.rs`)
   - Need to verify: should be JSON integer or JSON string?
   - Test with actual asciinema CLI output
   - Current code: `status.to_string()` - may be wrong

### Priority 2: Compliance Improvements

3. **Implement EOT support** (New feature)
   - Add `encode_eot()` to `streaming/alis.rs`
   - Send EOT when session ends but connection should persist
   - Update local ALiS endpoint to support multi-session streams

4. **Fix timestamp type** (Line 157 in `recording/asciicast_v3.rs`)
   ```rust
   // Current:
   header["timestamp"] = json!(time as i64);

   // Should be:
   let unix_timestamp = SystemTime::now()
       .duration_since(UNIX_EPOCH)?
       .as_secs();
   header["timestamp"] = json!(unix_timestamp);
   ```

### Priority 3: Testing

5. **Create golden test fixtures**
   - Download actual .cast file from asciinema.org
   - Verify exit event format
   - Compare our encoding against upstream

6. **Add ALiS protocol tests**
   - Byte-level validation against spec examples
   - Test EOT handling

## Upstream Sources Cited

1. **asciicast v3 Specification**: https://docs.asciinema.org/manual/asciicast/v3/
   - Defines header fields, event codes, data formats
   - Examples of NDJSON format

2. **ALiS v1 Protocol**: https://docs.asciinema.org/manual/server/streaming/
   - Binary event layouts with byte-level details
   - LEB128 encoding, theme formats, InitData semantics
   - Example byte sequences

3. **asciinema CLI** (3.x Rust rewrite): https://github.com/asciinema/asciinema
   - Reference implementation of recording and streaming
   - Source code for comparison

4. **asciinema Server**: https://github.com/asciinema/asciinema-server
   - WebSocket endpoint handling
   - Producer/consumer protocol translation

5. **asciinema Player**: https://github.com/asciinema/asciinema-player
   - Consumer-side ALiS v1 implementation
   - Defines player expectations

## Provenance Notes

All byte sequence examples and protocol details are from official asciinema documentation as of 2025-12-30. The specifications are normative; any deviations in our implementation are bugs unless explicitly documented as intentional design choices.
