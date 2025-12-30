use crate::session::Event;
use crate::streaming::alis;
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{connect_async_with_config, tungstenite::protocol::Message};

#[derive(Debug, Clone)]
pub enum StreamProtocol {
    Alis,
    AsciicastV3,
}

#[derive(Debug, Clone)]
pub struct StreamerConfig {
    pub server_url: String,
    pub install_id: Option<String>,
    pub install_id_path: Option<PathBuf>,
    pub title: Option<String>,
    pub visibility: Option<String>,
    pub protocol: StreamProtocol,
    pub capture_input: bool,
    pub theme: Option<alis::Theme>,
    pub term_type: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateStreamRequest {
    live: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    visibility: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateStreamResponse {
    ws_producer_url: String,
    #[allow(dead_code)]
    url: Option<String>,
    #[allow(dead_code)]
    id: Option<String>,
}

pub struct AsciinemaServerStreamer {
    config: StreamerConfig,
    event_id: u64,
    last_event_time: Option<Instant>,
    start_time: Instant,
}

impl AsciinemaServerStreamer {
    pub fn new(config: StreamerConfig) -> Self {
        Self {
            config,
            event_id: 0,
            last_event_time: None,
            start_time: Instant::now(),
        }
    }

    pub async fn run(&mut self, clients_tx: &mpsc::Sender<crate::session::Client>) -> Result<()> {
        // Get install ID
        let install_id = self.get_install_id()?;

        // Create stream
        let ws_url = self.create_stream(&install_id).await?;
        eprintln!("Connected to asciinema server");

        // Connect to WebSocket
        let (mut ws_stream, _) = self.connect_websocket(&ws_url).await?;

        // Send magic string for ALiS protocol
        if matches!(self.config.protocol, StreamProtocol::Alis) {
            ws_stream
                .send(Message::Binary(alis::ALIS_MAGIC.to_vec()))
                .await
                .context("failed to send ALiS magic")?;
        }

        // Subscribe to events
        let mut events = crate::session::stream(clients_tx).await?;

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok(event) => {
                    let messages = self.encode_event(event)?;
                    for msg in messages {
                        if let Err(e) = ws_stream.send(msg).await {
                            eprintln!("failed to send event to server: {}", e);
                            return Err(e.into());
                        }
                    }
                }
                Err(_) => {
                    // Lagged behind, continue
                    continue;
                }
            }
        }

        // Close WebSocket gracefully
        ws_stream.close(None).await.ok();

        Ok(())
    }

    fn get_install_id(&self) -> Result<String> {
        if let Some(id) = &self.config.install_id {
            return Ok(id.clone());
        }

        let path = if let Some(p) = &self.config.install_id_path {
            p.clone()
        } else {
            let home = std::env::var("HOME").context("HOME not set")?;
            PathBuf::from(home)
                .join(".config")
                .join("asciinema")
                .join("install-id")
        };

        std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read install-id from {:?}", path))
            .map(|s| s.trim().to_string())
    }

    async fn create_stream(&self, install_id: &str) -> Result<String> {
        let client = reqwest::Client::new();

        let api_url = format!("{}/api/v1/streams", self.config.server_url);

        let request_body = CreateStreamRequest {
            live: true,
            title: self.config.title.clone(),
            visibility: self.config.visibility.clone(),
        };

        let response = client
            .post(&api_url)
            .basic_auth("", Some(install_id))
            .json(&request_body)
            .send()
            .await
            .context("failed to create stream")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("stream creation failed ({}): {}", status, body);
        }

        let stream_info: CreateStreamResponse = response
            .json()
            .await
            .context("failed to parse stream response")?;

        if let Some(url) = &stream_info.url {
            eprintln!("Stream available at: {}", url);
        }

        Ok(stream_info.ws_producer_url)
    }

    async fn connect_websocket(
        &self,
        url: &str,
    ) -> Result<(
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        tokio_tungstenite::tungstenite::http::Response<Option<Vec<u8>>>,
    )> {
        let subprotocol = match self.config.protocol {
            StreamProtocol::Alis => "v1.alis",
            StreamProtocol::AsciicastV3 => "v3.asciicast",
        };

        let request = tokio_tungstenite::tungstenite::http::Request::builder()
            .uri(url)
            .header("Sec-WebSocket-Protocol", subprotocol)
            .body(())
            .context("failed to build WebSocket request")?;

        let ws_config = WebSocketConfig {
            max_message_size: Some(64 << 20), // 64 MB
            max_frame_size: Some(16 << 20),   // 16 MB
            ..Default::default()
        };

        let (stream, response) = connect_async_with_config(request, Some(ws_config), false)
            .await
            .context("failed to connect to WebSocket")?;

        Ok((stream, response))
    }

    fn encode_event(&mut self, event: Event) -> Result<Vec<Message>> {
        match self.config.protocol {
            StreamProtocol::Alis => self.encode_alis_event(event),
            StreamProtocol::AsciicastV3 => self.encode_v3_event(event),
        }
    }

    fn encode_alis_event(&mut self, event: Event) -> Result<Vec<Message>> {
        let mut messages = Vec::new();

        match event {
            Event::Init(_time, cols, rows, _pid, seq, _text) => {
                self.start_time = Instant::now();
                self.last_event_time = Some(self.start_time);

                let init_bytes = alis::encode_init(
                    self.event_id,
                    0, // rel_time is 0 for init
                    cols as u16,
                    rows as u16,
                    self.config.theme.as_ref(),
                    &seq,
                )?;

                messages.push(Message::Binary(init_bytes));
            }

            Event::Output(_time, data) => {
                self.event_id += 1;
                let rel_time = self.calculate_rel_time_micros();
                let bytes = alis::encode_output(self.event_id, rel_time, &data);
                messages.push(Message::Binary(bytes));
            }

            Event::Resize(_time, cols, rows) => {
                self.event_id += 1;
                let rel_time = self.calculate_rel_time_micros();
                let bytes = alis::encode_resize(self.event_id, rel_time, cols as u16, rows as u16);
                messages.push(Message::Binary(bytes));
            }

            Event::Marker(_time, label) => {
                self.event_id += 1;
                let rel_time = self.calculate_rel_time_micros();
                let bytes = alis::encode_marker(self.event_id, rel_time, &label);
                messages.push(Message::Binary(bytes));
            }

            Event::Input(_time, data) if self.config.capture_input => {
                self.event_id += 1;
                let rel_time = self.calculate_rel_time_micros();
                let bytes = alis::encode_input(self.event_id, rel_time, &data);
                messages.push(Message::Binary(bytes));
            }

            Event::Exit(_time, status) => {
                self.event_id += 1;
                let rel_time = self.calculate_rel_time_micros();
                let bytes = alis::encode_exit(self.event_id, rel_time, status);
                messages.push(Message::Binary(bytes));
            }

            Event::Snapshot(_, _, _, _) | Event::Input(_, _) => {
                // Ignore snapshots and input if not capturing
            }
        }

        Ok(messages)
    }

    fn encode_v3_event(&mut self, event: Event) -> Result<Vec<Message>> {
        let mut messages = Vec::new();

        match event {
            Event::Init(time, cols, rows, _pid, seq, _text) => {
                self.start_time = Instant::now();
                self.last_event_time = Some(self.start_time);

                // Send header
                let mut header = json!({
                    "version": 3,
                    "term": {
                        "cols": cols,
                        "rows": rows,
                    },
                    "timestamp": time as i64,
                });

                if let Some(term_type) = &self.config.term_type {
                    header["term"]["type"] = json!(term_type);
                }

                if let Some(theme) = &self.config.theme {
                    let mut theme_obj = json!({
                        "fg": theme.fg,
                        "bg": theme.bg,
                    });
                    if !theme.palette.is_empty() {
                        theme_obj["palette"] = json!(theme.palette);
                    }
                    header["term"]["theme"] = theme_obj;
                }

                if let Some(title) = &self.config.title {
                    header["title"] = json!(title);
                }

                messages.push(Message::Text(header.to_string()));

                // Send initial output at interval 0
                let event_line = json!([0.0, "o", seq]).to_string();
                messages.push(Message::Text(event_line));
            }

            Event::Output(_time, data) => {
                let interval = self.calculate_interval_secs();
                let event_line = json!([interval, "o", data]).to_string();
                messages.push(Message::Text(event_line));
            }

            Event::Resize(_time, cols, rows) => {
                let interval = self.calculate_interval_secs();
                let data = format!("{}x{}", cols, rows);
                let event_line = json!([interval, "r", data]).to_string();
                messages.push(Message::Text(event_line));
            }

            Event::Marker(_time, label) => {
                let interval = self.calculate_interval_secs();
                let event_line = json!([interval, "m", label]).to_string();
                messages.push(Message::Text(event_line));
            }

            Event::Input(_time, data) if self.config.capture_input => {
                let interval = self.calculate_interval_secs();
                let event_line = json!([interval, "i", data]).to_string();
                messages.push(Message::Text(event_line));
            }

            Event::Exit(_time, status) => {
                let interval = self.calculate_interval_secs();
                let status_str = status.to_string();
                let event_line = json!([interval, "x", status_str]).to_string();
                messages.push(Message::Text(event_line));
            }

            Event::Snapshot(_, _, _, _) | Event::Input(_, _) => {
                // Ignore
            }
        }

        Ok(messages)
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

    fn calculate_interval_secs(&mut self) -> f64 {
        let now = Instant::now();
        let secs = if let Some(last) = self.last_event_time {
            now.duration_since(last).as_secs_f64()
        } else {
            0.0
        };

        self.last_event_time = Some(now);
        secs
    }
}
