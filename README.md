# ht - headless terminal

`ht` (short for *headless terminal*) is a command line program that wraps an arbitrary other binary (e.g. `bash`, `vim`, etc.) with a VT100 style terminal interface--i.e. a pseudoterminal client (PTY) plus terminal server--and allows easy programmatic access to the input and output of that terminal (via JSON over STDIN/STDOUT). `ht` is built in rust and works on MacOS and Linux.

<img src="https://andykonwinski.com/assets/img/headless-terminal.png" alt="screenshot of raw terminal output vs ht output" align="right" style="width:450px">

## NEW: Recording & Streaming Features ✨

`ht` now includes native support for:
- **Recording sessions to asciicast v3 files** (.cast format)
- **Streaming to asciinema servers** (self-hosted or asciinema.org)
- **Local ALiS binary protocol** for high-performance streaming
- **Markers** for annotating recordings and streams
- **Exit events** for properly closed recordings
- **Optional input recording** (off by default for privacy)

## Use Cases & Motivation

`ht` is useful for programmatically interacting with terminals, which is important for programs that depend heavily on the Terminal as UI. It is useful for testing and for getting AI agents to interact with terminals the way humans do.

The original motiving use case was making terminals easy for LLMs to use. I was trying to use LLM agents for coding, and needed something like a **headless browser** but for terminals.

Terminals are one of the oldest and most prolific UI frameworks in all of computing. And they are stateful so, for example, when you use an editor in your terminal, the terminal has to manage state about the cursor location. Without ht, an agent struggles to manage this state directly; with ht, an agent can just observe the terminal like a human does.

## Installing

Download and use [the latest binary](https://github.com/andyk/ht/releases/latest) for your architecture.

## Building

Building from source requires the [Rust](https://www.rust-lang.org/) compiler
(1.84 or later), and the [Cargo package
manager](https://doc.rust-lang.org/cargo/). If they are not available via your
system package manager then use [rustup](https://rustup.rs/).

To download the source code, build the binary, and install it in
`$HOME/.cargo/bin` run:

```sh
cargo install --git https://github.com/andyk/ht
```

Then, ensure `$HOME/.cargo/bin` is in your shell's `$PATH`.

Alternatively, you can manually download the source code and build the binary
with:

```sh
git clone https://github.com/andyk/ht
cd ht
cargo build --release
```

This produces the binary in _release mode_ (`--release`) at
`target/release/ht`. There are no other build artifacts so you can just
copy the binary to a directory in your `$PATH`.

## Usage

### Basic Mode

Run `ht` to start interactive bash shell running in a PTY (pseudo-terminal).

To launch a different program (a different shell, another program) run `ht
<command> <args...>`. For example:

- `ht fish` - starts fish shell
- `ht nano` - starts nano editor
- `ht nano /etc/fstab` - starts nano editor with /etc/fstab opened

Default size of the virtual terminal window is 120x40 (cols by rows), which can
be changed with `--size` argument. For example: `ht --size 80x24`. The window
size can also be dynamically changed - see [resize command](#resize) below.

Run `ht -h` or `ht --help` to see all available options.

### Recording Mode

Record terminal sessions to asciicast v3 format (.cast files):

```sh
# Basic recording
ht record --out session.cast

# Record with title and idle time limiting
ht record --out demo.cast --title "My Demo" --idle-time-limit 2.0

# Record with input capture (WARNING: captures all keystrokes)
ht record --out full-session.cast --capture-input

# Record with theme customization
ht record --out themed.cast \
  --theme-fg "#ffffff" \
  --theme-bg "#000000" \
  --term-type xterm-256color

# Record and capture environment variables
ht record --out session.cast --capture-env "SHELL,TERM,USER"

# Append to existing recording
ht record --out session.cast --append

# Run specific command
ht record --out demo.cast bash -c "echo 'Hello, World!'"
```

**Recording features:**
- **asciicast v3 format**: Industry-standard format supported by asciinema player
- **Idle time limiting**: Automatically caps long pauses to keep recordings concise
- **Exit events**: Properly records process exit status
- **Markers**: Add annotations during recording (see [marker command](#mark))
- **Input recording**: Optionally capture keystrokes (off by default for privacy)
- **Theme support**: Customize terminal colors in recordings
- **Environment capture**: Selectively capture environment variables

### Streaming Mode

Stream terminal sessions to asciinema servers in real-time:

```sh
# Stream to asciinema.org (requires install-id)
ht stream --server https://asciinema.org --title "Live Demo"

# Stream with custom install-id
ht stream --server https://asciinema.org \
  --install-id-value "your-uuid-here" \
  --visibility unlisted

# Stream to self-hosted server using ALiS binary protocol (default)
ht stream --server https://your-server.com \
  --protocol alis \
  --title "Production Demo"

# Stream using asciicast v3 text protocol
ht stream --server https://your-server.com \
  --protocol v3 \
  --title "Debug Session"

# Stream with input capture
ht stream --server https://your-server.com --capture-input
```

**Streaming features:**
- **ALiS v1 binary protocol**: High-performance binary streaming (default)
- **asciicast v3 text protocol**: Alternative text-based streaming
- **Real-time streaming**: Events streamed as they happen
- **Automatic reconnection**: Graceful handling of network issues
- **Server authentication**: Uses asciinema install-id for authentication
- **Visibility control**: public, unlisted, or private streams

**Install-id location:**
By default, ht looks for install-id at `~/.config/asciinema/install-id`. You can override this with `--install-id-path` or provide the ID directly with `--install-id-value`.

## Live terminal preview

ht comes with a built-in HTTP server which provides a handy live terminal preview page and streaming endpoints.

To enable it, start ht with `-l` / `--listen` option. This will print the URL of
the live preview.

By default it listens on `127.0.0.1` and a system assigned, dynamic port. If you
need it to bind to another interface, or a specific port, pass the address to
the `-l` option, e.g. `-l 0.0.0.0:9999`.

### Local ALiS Binary Endpoint

When the HTTP server is enabled, ht exposes ALiS v1 binary protocol at `/ws/alis-v1`:

```sh
# Start ht with HTTP server
ht -l

# In another terminal, connect asciinema player to the local ALiS endpoint
# (Use the URL printed by ht, e.g., http://127.0.0.1:12345)
asciinema-player --stream ws://127.0.0.1:12345/ws/alis-v1
```

This endpoint provides:
- **High-performance binary streaming**: More efficient than JSON
- **`v1.alis` WebSocket subprotocol**: Standard ALiS protocol
- **Real-time updates**: Output, resize, marker, and exit events
- **Multiple consumers**: Multiple clients can connect simultaneously

## API

ht provides 2 types of API: STDIO and WebSocket.

The STDIO API allows control and introspection of the terminal using STDIN,
STDOUT and STDERR.

WebSocket API provides several endpoints for getting terminal updates in
real-time. Websocket API is _not_ enabled by default, and requires starting the
built-in HTTP server with `-l` / `--listen` option.

### STDIO API

ht uses simple JSON-based protocol for sending commands to its STDIN. Each
command must be sent on a separate line and be a JSON object having `"type"`
field set to one of the supported commands (below).

Some of the commands trigger [events](#events). ht may also internally trigger
various events on its own. To subscribe to desired events use `--subscribe
[<event-name>,<event-name>,...]` option when starting ht. This will print the
events as they occur to ht's STDOUT, as JSON-encoded objects. For example, to
subscribe to view snapshots (triggered by sending `takeSnapshot` command) use
`--subscribe snapshot` option. See [events](#events) below for a list of
available event types and their payloads.

Diagnostic messages (notices, errors) are printed to STDERR.

#### sendKeys

`sendKeys` command allows sending keys to a process running in the virtual
terminal as if the keys were pressed on a keyboard.

```json
{ "type": "sendKeys", "keys": ["nano", "Enter"] }
{ "type": "sendKeys", "keys": ["hello", "Enter", "world"] }
{ "type": "sendKeys", "keys": ["^x", "n"] }
```

Each element of the `keys` array can be either a key name or an arbitrary text.
If a key is not matched by any supported key name then the text is sent to the
process as is, i.e. like when using the `input` command.

The key and modifier specifications were inspired by
[tmux](https://github.com/tmux/tmux/wiki/Modifier-Keys).

The following key specifications are currently supported:

- `Enter`
- `Space`
- `Escape` or `^[` or `C-[`
- `Tab`
- `Left` - left arrow key
- `Right` - right arrow key
- `Up` - up arrow key
- `Down` - down arrow key
- `Home`
- `End`
- `PageUp`
- `PageDown`
- `F1` to `F12`

Modifier keys are supported by prepending a key with one of the prefixes:

- `^` - control - e.g. `^c` means <kbd>Ctrl</kbd> + <kbd>C</kbd>
- `C-` - control - e.g. `C-c` means <kbd>Ctrl</kbd> + <kbd>C</kbd>
- `S-` - shift - e.g. `S-F6` means <kbd>Shift</kbd> + <kbd>F6</kbd>
- `A-` - alt/option - e.g. `A-Home` means <kbd>Alt</kbd> + <kbd>Home</kbd>

Modifiers can be combined (for arrow keys only at the moment), so combinations
such as `S-A-Up` or `C-S-Left` are possible.

`C-` control modifier notation can be used with ASCII letters (both lower and
upper case are supported) and most special key names. The caret control notation
(`^`) may only be used with ASCII letters, not with special keys.

Shift modifier can be used with special key names only, such as `Left`, `PageUp`
etc. For text characters, instead of specifying e.g. `S-a` just use upper case
`A`.

Alt modifier can be used with any Unicode character and most special key names.

This command doesn't trigger any event.

#### input

`input` command allows sending arbitrary raw input to a process running in the
virtual terminal.

```json
{ "type": "input", "payload": "ls\r" }
```

In most cases it's easier and recommended to use the `sendKeys` command instead.

Use the `input` command if you don't want any special input processing, i.e. no
mapping of key names to their respective control sequences.

For example, to send Ctrl-C shortcut you must use `"\u0003"` (0x03) as the
payload:

```json
{ "type": "input", "payload": "\u0003" }
```

This command doesn't trigger any event.

#### mark

**NEW**: `mark` command allows adding markers to recordings and streams. Markers are useful for annotating key moments in terminal sessions.

```json
{ "type": "mark", "label": "Step 1: Setup" }
{ "type": "mark", "label": "Checkpoint" }
{ "type": "mark", "label": "" }
```

Markers are:
- Recorded to asciicast files as `["m", "label"]` events
- Streamed to asciinema servers as marker events
- Broadcast to WebSocket consumers
- Supported by asciinema player for navigation

This command triggers `marker` event.

#### takeSnapshot

`takeSnapshot` command allows taking a textual snapshot of the the terminal view.

```json
{ "type": "takeSnapshot" }
```

This command triggers `snapshot` event.

#### resize

`resize` command allows resizing the virtual terminal window dynamically by
specifying new width (`cols`) and height (`rows`).

```json
{ "type": "resize", "cols": 80, "rows": 24 }
```

This command triggers `resize` event.

### WebSocket API

The WebSocket API currently provides 3 endpoints:

#### `/ws/events`

This endpoint allows the client to subscribe to events that happen in ht.

Query param `sub` should be set to a comma-separated list of desired events.
E.g. `/ws/events?sub=init,snapshot`.

Events are delivered as JSON encoded strings, using WebSocket text message type.

See [events](#events) section below for the description of all available events.

#### `/ws/alis`

This endpoint implements JSON flavor of [asciinema live stream
protocol](https://github.com/asciinema/asciinema-player/blob/develop/src/driver/websocket.js),
therefore allows pointing asciinema player directly to ht to get a real-time
terminal preview. This endpoint is used by the live terminal preview page
mentioned above.

#### `/ws/alis-v1`

**NEW**: This endpoint implements ALiS v1 binary protocol for high-performance streaming to consumers.

- **Protocol**: `v1.alis` WebSocket subprotocol
- **Format**: Binary messages with LEB128-encoded events
- **Features**: Init with snapshot, Output, Resize, Marker, Exit events
- **Use case**: Connecting asciinema player or other ALiS consumers

### Events

The events emitted to STDOUT and via `/ws/events` WebSocket endpoint are
identical, i.e. they are JSON-encoded objects with the same fields and payloads.

Every event contains 2 top-level fields:

- `type` - type of event,
- `data` - associated data, specific to each event type.

The following event types are currently available:

#### `init`

Same as `snapshot` event (see below) but sent only once, as the first event
after ht's start (when sent to STDOUT) and upon establishing of WebSocket
connection.

In addition to the fields from `snapshot` event this one includes:

- `pid` - PID of the top-level process started by ht (e.g. PID of bash)

#### `output`

Terminal output. Sent when an application (e.g. shell) running under ht prints
something to the terminal.

Event data is an object with the following fields:

- `seq` - a raw sequence of characters written to a terminal, potentially including control sequences (colors, cursor positioning, etc.)

#### `resize`

Terminal resize. Send when the terminal is resized with the `resize` command.

Event data is an object with the following fields:

- `cols` - current terminal width, number of columns
- `rows` - current terminal height, number of rows

#### `snapshot`

Terminal window snapshot. Sent when the terminal snapshot is taken with the
`takeSnapshot` command.

Event data is an object with the following fields:

- `cols` - current terminal width, number of columns
- `rows` - current terminal height, number of rows
- `text` - plain text snapshot as multi-line string, where each line represents a terminal row
- `seq` - a raw sequence of characters, which when printed to a blank terminal puts it in the same state as [ht's virtual terminal](https://github.com/asciinema/avt)

#### `marker`

**NEW**: Marker event. Sent when a marker is added using the `mark` command.

Event data is an object with the following fields:

- `label` - marker label (string, may be empty)

#### `input`

**NEW**: Input event. Sent when input recording is enabled (`--capture-input`) and input is sent to the terminal.

**⚠️ Privacy Warning**: Input events capture all keystrokes including passwords and sensitive data. Only enable with `--capture-input` when necessary.

Event data is an object with the following fields:

- `data` - raw input data sent to the terminal

#### `exit`

**NEW**: Exit event. Sent when the wrapped process exits.

Event data is an object with the following fields:

- `status` - process exit status code (0 for success, non-zero for error)

## Examples

### Recording a Demo Session

```sh
# Start recording
ht record --out demo.cast --title "CLI Demo" --idle-time-limit 2.0

# In the terminal, perform your demo
$ echo "Hello, World!"
Hello, World!

# Send a marker (from another terminal)
echo '{"type":"mark","label":"Important Step"}' | nc localhost 5000

# Exit the shell to finish recording
$ exit
```

### Streaming to asciinema.org

```sh
# Get your install-id from asciinema
asciinema auth

# Stream your session
ht stream --server https://asciinema.org \
  --title "Live Debugging Session" \
  --visibility unlisted

# The URL will be printed - share it with collaborators
# Stream available at: https://asciinema.org/s/abc123
```

### Using Markers for Navigation

```sh
# Terminal 1: Start recording with HTTP server
ht record --out tutorial.cast -l

# Terminal 2: Run your commands and add markers
curl http://localhost:PORT -d '{"type":"mark","label":"Step 1: Installation"}'
# ... perform installation ...
curl http://localhost:PORT -d '{"type":"mark","label":"Step 2: Configuration"}'
# ... configure ...
curl http://localhost:PORT -d '{"type":"mark","label":"Step 3: Testing"}'
# ... run tests ...
```

### Privacy-Conscious Recording

By default, ht does NOT record input for privacy reasons. Only enable input recording when necessary:

```sh
# Safe: No input recording (default)
ht record --out safe-demo.cast

# With input: Captures ALL keystrokes including passwords!
ht record --out full-session.cast --capture-input
```

## Testing on command line

ht is aimed at programmatic use given its JSON-based API, however one can play
with it by just launching it in a normal desktop terminal emulator and typing in
JSON-encoded commands from keyboard and observing the output on STDOUT.

[rlwrap](https://github.com/hanslub42/rlwrap) can be used to wrap STDIN in a
readline based editable prompt, which also provides history (up/down arrows).

To use `rlwrap` with `ht`:

```sh
rlwrap ht [ht-args...]
```

## Python and Typescript libs

Here are some experimental versions of a simple Python and Typescript libraries that wrap `ht`: [htlib.py](https://github.com/andyk/headlong/blob/24e9e5f37b79b3a667774eefa3a724b59b059775/packages/env/htlib.py) and a [htlib.ts](https://github.com/andyk/headlong/blob/24e9e5f37b79b3a667774eefa3a724b59b059775/packages/env/htlib.ts).

TODO: either pull those into this repo or fork them into their own `htlib` repo.

## Architecture

### Event Bus

All events flow through an internal broadcast channel that allows multiple consumers:
- STDIO subscribers (via `--subscribe`)
- WebSocket `/ws/events` endpoint
- WebSocket `/ws/alis` JSON endpoint
- WebSocket `/ws/alis-v1` binary endpoint
- Recording to .cast files
- Streaming to asciinema servers

This architecture ensures:
- **No blocking**: File I/O and network operations don't block the PTY
- **Backpressure handling**: Bounded channels prevent unbounded memory growth
- **Multiple consumers**: Many clients can observe the same session
- **Consistent events**: All consumers see the same event stream

### Recording Format

ht implements asciicast v3 specification:
- **NDJSON format**: One JSON object per line
- **Header**: Terminal configuration, metadata, theme
- **Events**: `[interval, code, data]` tuples
- **Codes**: `o` (output), `r` (resize), `m` (marker), `i` (input), `x` (exit)
- **Intervals**: Time since previous event (not absolute timestamps)

### Streaming Protocols

#### ALiS v1 Binary (Preferred)

- **Magic**: `ALiS\x01` (5 bytes)
- **Encoding**: LEB128 for integers, length-prefixed strings
- **Events**: Init(0x01), Output(0x6F), Input(0x69), Resize(0x72), Marker(0x6D), Exit(0x78)
- **Timing**: Microseconds since previous event
- **Benefits**: Compact, efficient, widely supported

#### asciicast v3 Text

- **Format**: NDJSON over WebSocket text frames
- **Header**: First message contains terminal config
- **Events**: JSON arrays `[interval, code, data]`
- **Benefits**: Human-readable, easy to debug

## Possible future work

* ~~native integration with asciinema for recording terminal sessions~~ ✅ **DONE**
* update the interface to return the view with additional color and style information (text color, background, bold/italic/etc) also in a simple JSON format (so no dealing with color-related escape sequence either), and the frontend could render this using HTML (e.g. with styled pre/span tags, similar to how asciinema-player does it) or with SVG.
* ~~support subscribing to view updates, to avoid needing to poll~~ ✅ **DONE** (via WebSocket endpoints)
* Recording to other formats (GIF, MP4)
* OSC theme detection for automatic theme capture
* Replay mode for .cast files

## Alternatives and related projects

[`expect`](https://core.tcl-lang.org/expect/index) is an old related tool that let's you `spawn` an arbitrary binary and then `send` input to it and specify what output you `expect` it to generate next.

[`asciinema`](https://asciinema.org/) is the standard tool for recording terminal sessions. ht now provides native asciicast recording and streaming, making it easy to integrate terminal recording into automated workflows.

Also, note that if there exists an explicit API to achieve your given task (e.g. a library that comes with the tool you're targeting), it will probably be less bug prone/finicky to use the API directly rather than working with your tool through `ht`.

See also [this hackernews discussion](https://news.ycombinator.com/item?id=40552257) where a bunch of other tools were discussed!

## Design doc

Here is [the original design doc](https://docs.google.com/document/d/1L1prpWos3gIYTkfCgeZ2hLScypkA73WJ9KxME5NNbNk/edit) we used to drive the project development.

## License

All code is licensed under the Apache License, Version 2.0. See LICENSE file for
details.
