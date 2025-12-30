# Implementation Audit: ht Recording & Streaming

**Date**: 2025-12-30
**Version**: Post-initial implementation
**Status**: AUDIT IN PROGRESS

## 1. Event Pipeline Architecture

### 1.1 Event Production Sites

**Source**: `src/session.rs`

Events are produced in `Session` struct:
- **Init**: Line 79-93 in `Session::subscribe()` - Created on-demand when client subscribes
  - Contains: time, cols, rows, pid, vt.dump() (seq), text_view()
  - Timing: `elapsed_time()` computed as `stream_time + last_event_time.elapsed()`

- **Output**: Line 48-54 in `Session::output()`
  - Triggered by: PTY data in `main.rs:370` (`session.output()`)
  - Contains: time (start_time.elapsed()), data (String)
  - Updates: `stream_time`, `last_event_time`

- **Resize**: Line 56-62 in `Session::resize()`
  - Triggered by: STDIO resize command in `main.rs:379`
  - Contains: time, cols, rows
  - Updates: `stream_time`, `last_event_time`

- **Marker**: Line 78-83 in `Session::marker()`
  - Triggered by: STDIO mark command in `main.rs:382`
  - Contains: time, label (String)
  - Updates: `stream_time`, `last_event_time`

- **Input**: Line 85-90 in `Session::input()`
  - Triggered by: Only when capture_input=true in `main.rs:371-375`
  - Contains: time, data (String)
  - Updates: `stream_time`, `last_event_time`

- **Exit**: Line 92-97 in `Session::exit()`
  - Triggered by: PTY task completion in `main.rs:392-407`
  - Contains: time, status (i32)
  - Updates: `stream_time`, `last_event_time`

- **Snapshot**: Line 67-76 in `Session::snapshot()`
  - Triggered by: STDIO takeSnapshot command
  - Contains: cols, rows, vt.dump(), text_view()
  - Does NOT update timing (no time field)

### 1.2 Event Flow (Text Diagram)

```
PTY (pty.rs)
  ↓ output_tx (mpsc::channel)
main.rs:event_loop
  ↓ session.output()/resize()/marker()/exit()
Session (session.rs)
  ↓ broadcast_tx.send(Event)
tokio::broadcast::channel (1024 capacity)
  ↓ (multiple subscribers via broadcast_rx)
  ├─→ STDIO API (api/stdio.rs)
  │     ↓ to_json() → println!()
  ├─→ /ws/events (api/http.rs)
  │     ↓ event_stream_message() → WebSocket text
  ├─→ /ws/alis (api/http.rs)
  │     ↓ alis_message() → WebSocket JSON text
  ├─→ /ws/alis-v1 (streaming/alis_local.rs)
  │     ↓ convert_to_alis_binary() → WebSocket binary
  ├─→ AsciicastV3Recorder (recording/asciicast_v3.rs)
  │     ↓ handle_event() → File (BufWriter)
  └─→ AsciinemaServerStreamer (streaming/asciinema_server.rs)
        ↓ encode_alis_event()/encode_v3_event() → Remote WebSocket
```

### 1.3 Threading Model

**Source**: `src/main.rs`

- **Main Task**: Event loop (`run_event_loop`) - Lines 326-416
  - Receives: PTY output, STDIO commands, client subscriptions
  - Calls: `session.*()` methods which send to broadcast channel
  - Non-blocking: all sends are bounded channel operations

- **PTY Task**: `pty::spawn` returns Future - Line 293
  - Runs in: `tokio::spawn(fut)` at Line 293
  - Sends: Raw bytes via `output_tx`
  - Blocks on: PTY read (tokio AsyncFd)

- **STDIO API Task**: `api::stdio::start` - Line 284
  - Runs in: `tokio::spawn(...)` at Line 284
  - Receives: Commands from stdin (blocking thread → mpsc unbounded)
  - Subscribes to: Events via `session::stream()`

- **HTTP Server Task**: `api::http::start` - Line 319
  - Runs in: `tokio::spawn(...)` at Line 319
  - Each WebSocket: Separate task in axum handler

- **Recorder Task**: `recorder.run()` - Line 124
  - Runs in: `tokio::spawn(...)` at Line 124
  - Subscribes to: Events via `session::stream()`
  - Blocks on: File I/O (BufWriter::flush)

- **Streamer Task**: `streamer.run()` - Line 215
  - Runs in: `tokio::spawn(...)` at Line 215
  - Subscribes to: Events via `session::stream()`
  - Blocks on: Network I/O (WebSocket send)

**Threading Issues Identified**:
1. ❌ Recorder blocks on flush after every event (Line 191 in asciicast_v3.rs)
   - Could slow down PTY if file I/O is slow
   - BUT: runs in separate task, so only affects recording
   - Broadcast channel is bounded (1024), so could lag/drop events

2. ❌ Streamer blocks on network send for each event
   - WebSocket send is async but could block on slow network
   - Bounded broadcast channel means lagged events are dropped

3. ✅ Lagged event handling: Both recorder and streamer catch `Err(_)` and continue (Lines 72-75 in asciicast_v3.rs)

## 2. Timing Correctness

### 2.1 Time Source

**Source**: `src/session.rs`

- **Clock**: `std::time::Instant` (monotonic, steady clock) ✅
- **Start Time**: Set in `Session::new()` at Line 42
- **Event Time**: Computed as `start_time.elapsed().as_secs_f64()` (Lines 50, 58, 79)

### 2.2 Delta Computation

**asciicast v3** (`src/recording/asciicast_v3.rs`):
- Line 196-210: `calculate_interval()`
- Computes: `now.duration_since(last).as_secs_f64()` ✅
- First event: interval = 0.0 (last_event_time is None) ✅
- Applied idle limit: `interval.min(limit)` at Line 208 ✅
- **Issue**: ❌ No check for negative deltas (should be impossible with Instant, but worth asserting)

**ALiS v1 binary** (`src/streaming/asciinema_server.rs`):
- Line 299-309: `calculate_rel_time_micros()`
- Computes: `now.duration_since(last).as_micros() as u64` ✅
- First event (Init): rel_time = 0 at Line 246 ✅
- **Issue**: ❌ No check for u64 overflow (unlikely but possible with very long-running sessions)

**ALiS v1 text** (`src/streaming/asciinema_server.rs`):
- Line 311-321: `calculate_interval_secs()`
- Same as asciicast v3 ✅

### 2.3 Burst Output Handling

**Issue**: ❌ No explicit handling of burst output
- If PTY emits 1000 lines rapidly, each gets its own timestamp
- Deltas will be very small (microseconds)
- This is correct behavior ✅
- BUT: Broadcast channel can lag if consumers are slow

### 2.4 Proposed Assertions (Debug Mode)

```rust
#[cfg(debug_assertions)]
{
    assert!(interval >= 0.0, "Negative time delta detected");
    assert!(interval.is_finite(), "Non-finite time delta");
}
```

## 3. Protocol Correctness

### 3.1 asciicast v3 Format

**Source**: `src/recording/asciicast_v3.rs`

**Header** (Lines 144-183):
- ✅ Required: `version: 3`, `term: {cols, rows}`
- ✅ Optional: `timestamp` (unix seconds)
- ✅ Optional: `idle_time_limit`
- ✅ Optional: `command`, `title`
- ✅ Optional: `env` (Map<String, String>)
- ✅ Optional: `term.type` (e.g., "xterm-256color")
- ✅ Optional: `term.theme` (fg, bg, palette)

**Events** (Lines 185-194):
- Format: `[interval_seconds, code, data]` ✅
- Codes:
  - "o" → output (data = raw text) ✅
  - "r" → resize (data = "COLSxROWS") ✅
  - "m" → marker (data = label) ✅
  - "i" → input (data = raw input) ✅
  - "x" → exit (data = status as string) ✅

**Issues**:
1. ❌ First output event at interval 0.0 (Line 95)
   - Need to verify: Does asciinema CLI do this?
   - Or should init data be separate from first output?

2. ✅ Flush after every event (Line 192)
   - Safe for crash recovery
   - May be performance issue (see timing section)

### 3.2 ALiS v1 Binary Protocol

**Source**: `src/streaming/alis.rs`, `src/streaming/asciinema_server.rs`

**Magic String** (Line 246 in asciinema_server.rs):
- ✅ Sends `b"ALiS\x01"` as first binary message

**Event Framing**:
- ✅ One event per WebSocket binary message
- ✅ Event type byte + LEB128-encoded fields

**LEB128 Encoding** (Lines 37-54 in alis.rs):
- ✅ Implements unsigned LEB128
- ✅ Tests: 0, 1, 127, 128, 300, 16384 (Line 233-238)

**String Encoding** (Lines 57-62 in alis.rs):
- ✅ Length prefix (LEB128) + UTF-8 bytes
- ✅ Tests: "", "a", "hello" (Line 241-244)

**Event Types**:
- Init (0x01): ✅ LastId, RelTime, Cols, Rows, Theme, InitData
- Output (0x6F): ✅ Id, RelTime, Data
- Input (0x69): ✅ Id, RelTime, Data
- Resize (0x72): ✅ Id, RelTime, Cols, Rows
- Marker (0x6D): ✅ Id, RelTime, Label
- Exit (0x78): ✅ Id, RelTime, Status

**Issues**:
1. ❌ **NO EOT (End of Transmission) support**
   - Spec allows streams to send EOT to keep connection open across restarts
   - Not implemented in our code

2. ❓ **Theme encoding** (Lines 80-124 in alis.rs):
   - Implements Format 0x00 (none), 0x08 (8-color), 0x10 (16-color)
   - Color parsing: `#RRGGBB` → [R, G, B] bytes ✅
   - **Need to verify**: Exact byte layout against spec

3. ❓ **Init LastId and RelTime semantics**:
   - We use LastId=0, RelTime=0 for Init (Line 244 in asciinema_server.rs)
   - Need to verify: Should LastId be the last event ID from previous session?

### 3.3 WebSocket Subprotocol Negotiation

**Remote Streaming** (`src/streaming/asciinema_server.rs`):
- Line 181-189: Sets `Sec-WebSocket-Protocol` header ✅
  - "v1.alis" for ALiS binary
  - "v3.asciicast" for v3 text

**Local ALiS Endpoint** (`src/api/http.rs`):
- Line 73: Uses `.protocols(["v1.alis"])` ✅
- **Issue**: ❓ Need to verify this actually negotiates the subprotocol correctly

## 4. Failure Modes & Policies

### 4.1 File Write Blocking

**Recorder** (`src/recording/asciicast_v3.rs`):
- Runs in: Separate tokio task (Line 124 in main.rs)
- Blocks on: `writer.flush()` after every event (Line 192)
- **Policy**:
  - ✅ If file I/O is slow, recorder task falls behind
  - ✅ Broadcast channel is bounded (1024), so lagged events are dropped (Line 72-75)
  - ✅ Recorder continues on lag (doesn't crash main loop)
- **Concern**: ❌ Silent event loss if recorder is too slow

### 4.2 Network Send Blocking/Failure

**Streamer** (`src/streaming/asciinema_server.rs`):
- Runs in: Separate tokio task (Line 215 in main.rs)
- Blocks on: `ws_stream.send(msg).await` (Line 67-71)
- **Policy**:
  - ❌ If send fails, returns error and exits streamer task (Line 68)
  - ❌ Main session continues, but streaming stops silently
  - ✅ Lagged events are dropped (Line 73-75)

### 4.3 Mid-Session Consumer Connection

**Local ALiS Endpoint** (`src/streaming/alis_local.rs`):
- Init event: Line 136-142 in `convert_to_alis_binary()`
  - ✅ Sends Init with InitData (current snapshot)
  - ✅ Late joiner gets current state
- **Issue**: ❓ Need to verify InitData actually contains usable snapshot

**HTTP /ws/alis JSON** (`src/api/http.rs`):
- Line 104-109: Sends Init on connection
- ✅ Contains current snapshot from `vt.dump()`

### 4.4 Session Restart with Persistent Consumer

**Issue**: ❌ NOT IMPLEMENTED
- If PTY exits and restarts, consumer sees connection close
- No EOT support to keep connection open

### 4.5 Memory Pressure from Large Output

**Broadcast Channel**:
- Capacity: 1024 messages (Line 35 in session.rs)
- **Policy**: ✅ Lagged receivers drop old messages
- **Issue**: ❌ Each event contains full output string
  - A 1MB output line is stored 1024 times if all subscribers lag
  - Could cause OOM on huge outputs

**Proposed**: Use `Arc<String>` or `Bytes` for event data to share memory

## 5. Outstanding Questions (Requires Upstream Comparison)

1. ❓ Does asciinema CLI emit init data as a separate output event at t=0?
2. ❓ What is the exact byte layout for ALiS theme encoding?
3. ❓ Should ALiS Init use non-zero LastId for reconnections?
4. ❓ Do we need EOT support for production use?
5. ❓ How does asciinema CLI handle idle_time_limit? (cap each delta? or post-process?)
6. ❓ Are there required metadata fields we're missing?
7. ❓ How should markers be represented in player UI?
8. ❓ Exit event: string status or integer? Spec says integer, we use string for v3.

## 6. Checklist for Runtime Validation

Debug-mode assertions to add:

```rust
// In asciicast_v3.rs::calculate_interval()
debug_assert!(interval >= 0.0);
debug_assert!(interval.is_finite());

// In asciinema_server.rs::calculate_rel_time_micros()
debug_assert!(rel_time < u64::MAX / 2); // Catch potential overflow

// In alis.rs::encode_leb128()
debug_assert!(result.len() <= 10); // Max 10 bytes for u64

// In session.rs::output()/resize()/marker()
let old_time = self.stream_time;
// ... update stream_time ...
debug_assert!(self.stream_time >= old_time);
```

## 7. Code References

All line numbers refer to current implementation as of commit 27c64f2.

Key files:
- `src/session.rs` - Event generation and broadcast
- `src/main.rs` - Event loop and task spawning
- `src/recording/asciicast_v3.rs` - asciicast v3 writer
- `src/streaming/alis.rs` - ALiS v1 encoder
- `src/streaming/asciinema_server.rs` - Remote streaming client
- `src/streaming/alis_local.rs` - Local ALiS endpoint
- `src/api/http.rs` - WebSocket endpoints

## Next Steps

1. Fetch upstream asciinema sources
2. Compare our event schemas against upstream
3. Create golden test fixtures
4. Address all ❌ and ❓ items above
