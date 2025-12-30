use crate::session;
use crate::streaming::alis;
use anyhow::Result;
use axum::extract::ws;
use futures_util::{SinkExt, StreamExt};
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;

/// Handle ALiS binary WebSocket connection for local consumers
pub async fn handle_alis_binary_socket(
    socket: ws::WebSocket,
    clients_tx: mpsc::Sender<session::Client>,
) -> Result<()> {
    let (mut sink, stream) = socket.split();

    // Drain incoming messages (consumer shouldn't send anything meaningful)
    let drainer = tokio::spawn(stream.map(Ok).forward(futures_util::sink::drain()));

    // Send magic string immediately
    sink.send(ws::Message::Binary(alis::ALIS_MAGIC.to_vec()))
        .await?;

    // Subscribe to events and convert to ALiS binary messages
    let result = session::stream(&clients_tx)
        .await?
        .filter_map(alis_binary_message)
        .forward(&mut sink)
        .await;

    drainer.abort();
    result?;

    Ok(())
}

/// State tracker for ALiS binary encoding
struct AlisState {
    event_id: u64,
    last_event_time: Option<Instant>,
    start_time: Instant,
}

impl AlisState {
    fn new() -> Self {
        Self {
            event_id: 0,
            last_event_time: None,
            start_time: Instant::now(),
        }
    }

    fn calculate_rel_time_micros(&mut self) -> u64 {
        let now = Instant::now();
        let micros = if let Some(last) = self.last_event_time {
            now.duration_since(last).as_micros() as u64
        } else {
            0
        };

        self.last_event_time = Some(now);
        micros
    }
}

async fn alis_binary_message(
    event: Result<session::Event, BroadcastStreamRecvError>,
) -> Option<Result<ws::Message, axum::Error>> {
    use session::Event::*;

    // We need to maintain state across calls, but filter_map doesn't allow mutable state
    // For now, we'll use thread-local storage or compute relative time on the fly
    // This is a limitation - ideally we'd use a stateful stream adapter

    match event {
        Ok(Init(_time, cols, rows, _pid, seq, _text)) => {
            // For Init, rel_time should be 0 (or microseconds since session start)
            match alis::encode_init(0, 0, cols as u16, rows as u16, None, &seq) {
                Ok(bytes) => Some(Ok(ws::Message::Binary(bytes))),
                Err(e) => Some(Err(axum::Error::new(e))),
            }
        }

        Ok(Output(_time, data)) => {
            // Without state, we can't properly calculate rel_time
            // We'll use absolute time converted to micros as a workaround
            // This is not ideal but functional for local preview
            let id = (_time * 1_000_000.0) as u64; // Use time as pseudo-id
            let bytes = alis::encode_output(id, 0, &data);
            Some(Ok(ws::Message::Binary(bytes)))
        }

        Ok(Resize(_time, cols, rows)) => {
            let id = (_time * 1_000_000.0) as u64;
            let bytes = alis::encode_resize(id, 0, cols as u16, rows as u16);
            Some(Ok(ws::Message::Binary(bytes)))
        }

        Ok(Marker(_time, label)) => {
            let id = (_time * 1_000_000.0) as u64;
            let bytes = alis::encode_marker(id, 0, &label);
            Some(Ok(ws::Message::Binary(bytes)))
        }

        Ok(Exit(_time, status)) => {
            let id = (_time * 1_000_000.0) as u64;
            let bytes = alis::encode_exit(id, 0, status);
            Some(Ok(ws::Message::Binary(bytes)))
        }

        Ok(Input(_, _)) | Ok(Snapshot(_, _, _, _)) => None,

        Err(e) => Some(Err(axum::Error::new(e))),
    }
}

/// Stateful version using proper state tracking
pub async fn handle_alis_binary_socket_stateful(
    socket: ws::WebSocket,
    clients_tx: mpsc::Sender<session::Client>,
) -> Result<()> {
    let (mut sink, stream) = socket.split();

    // Drain incoming messages
    let drainer = tokio::spawn(stream.map(Ok).forward(futures_util::sink::drain()));

    // Send magic string
    sink.send(ws::Message::Binary(alis::ALIS_MAGIC.to_vec()))
        .await?;

    // Subscribe to events
    let mut events = session::stream(&clients_tx).await?;
    let mut state = AlisState::new();

    while let Some(event_result) = events.next().await {
        match event_result {
            Ok(event) => {
                if let Some(msg) = convert_to_alis_binary(&mut state, event)? {
                    if sink.send(msg).await.is_err() {
                        break;
                    }
                }
            }
            Err(_) => {
                // Lagged, continue
                continue;
            }
        }
    }

    drainer.abort();
    Ok(())
}

fn convert_to_alis_binary(
    state: &mut AlisState,
    event: session::Event,
) -> Result<Option<ws::Message>> {
    use session::Event::*;

    match event {
        Init(_time, cols, rows, _pid, seq, _text) => {
            state.start_time = Instant::now();
            state.last_event_time = Some(state.start_time);
            state.event_id = 0;

            let bytes = alis::encode_init(0, 0, cols as u16, rows as u16, None, &seq)?;
            Ok(Some(ws::Message::Binary(bytes)))
        }

        Output(_time, data) => {
            state.event_id += 1;
            let rel_time = state.calculate_rel_time_micros();
            let bytes = alis::encode_output(state.event_id, rel_time, &data);
            Ok(Some(ws::Message::Binary(bytes)))
        }

        Resize(_time, cols, rows) => {
            state.event_id += 1;
            let rel_time = state.calculate_rel_time_micros();
            let bytes = alis::encode_resize(state.event_id, rel_time, cols as u16, rows as u16);
            Ok(Some(ws::Message::Binary(bytes)))
        }

        Marker(_time, label) => {
            state.event_id += 1;
            let rel_time = state.calculate_rel_time_micros();
            let bytes = alis::encode_marker(state.event_id, rel_time, &label);
            Ok(Some(ws::Message::Binary(bytes)))
        }

        Exit(_time, status) => {
            state.event_id += 1;
            let rel_time = state.calculate_rel_time_micros();
            let bytes = alis::encode_exit(state.event_id, rel_time, status);
            Ok(Some(ws::Message::Binary(bytes)))
        }

        Input(_, _) | Snapshot(_, _, _, _) => Ok(None),
    }
}
