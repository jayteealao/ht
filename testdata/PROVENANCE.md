# Test Data Provenance

## golden_minimal.cast

**Source**: Manually constructed based on [asciicast v3 specification](https://docs.asciinema.org/manual/asciicast/v3/)
**Date**: 2025-12-30
**Purpose**: Minimal valid asciicast v3 file for testing

**Key features**:
- All required header fields (version, term.cols, term.rows)
- Optional timestamp
- Multiple event types: output (o), marker (m), resize (r), exit (x)
- Exit event with INTEGER status (not string) - this is critical to verify
- ANSI escape sequences in output
- Delta timing (intervals between events)

**Notes**:
1. Exit event data is `0` (JSON integer), NOT `"0"` (string)
2. Intervals are floats representing seconds since previous event
3. Marker can have empty or non-empty label
4. Resize format is exactly "COLSxROWS" as string

## alis_init_example.bin

**Source**: Manually constructed based on [ALiS v1 specification](https://docs.asciinema.org/manual/server/streaming/)
**Date**: 2025-12-30
**Purpose**: Example ALiS Init event for byte-level testing

Binary representation of:
```
Init event:
  EventType: 0x01
  LastId: 0
  RelTime: 0
  Cols: 80
  Rows: 24
  Theme: None (0x00)
  InitData: "Hello!"
```

Expected bytes: `\x01\x00\x00\x50\x18\x00\x06Hello!`

## Future Test Data

To add:
- [ ] Actual .cast file downloaded from asciinema.org
- [ ] .cast file with input events
- [ ] .cast file with theme metadata
- [ ] ALiS binary stream capture from asciinema server
- [ ] Edge cases: very long outputs, unicode, etc.
