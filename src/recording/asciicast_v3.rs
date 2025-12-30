use crate::session::Event;
use anyhow::{Context, Result};
use serde_json::json;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

#[derive(Debug, Clone)]
pub struct RecorderConfig {
    pub output_path: PathBuf,
    pub append: bool,
    pub idle_time_limit: Option<f64>,
    pub title: Option<String>,
    pub command: Option<String>,
    pub capture_env: Vec<String>,
    pub theme: Option<ThemeConfig>,
    pub term_type: Option<String>,
    pub capture_input: bool,
}

#[derive(Debug, Clone)]
pub struct ThemeConfig {
    pub fg: String,
    pub bg: String,
    pub palette: Option<String>,
}

pub struct AsciicastV3Recorder {
    writer: BufWriter<File>,
    config: RecorderConfig,
    last_event_time: Option<Instant>,
    start_time: Instant,
    header_written: bool,
}

impl AsciicastV3Recorder {
    pub fn new(config: RecorderConfig) -> Result<Self> {
        let file = if config.append {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&config.output_path)
                .context("failed to open recording file")?
        } else {
            File::create(&config.output_path).context("failed to create recording file")?
        };

        Ok(Self {
            writer: BufWriter::new(file),
            config,
            last_event_time: None,
            start_time: Instant::now(),
            header_written: false,
        })
    }

    pub async fn run(
        &mut self,
        clients_tx: &mpsc::Sender<crate::session::Client>,
    ) -> Result<()> {
        let mut events = crate::session::stream(clients_tx).await?;

        while let Some(event_result) = events.next().await {
            match event_result {
                Ok(event) => {
                    self.handle_event(event)?;
                }
                Err(_) => {
                    // Lagged behind, continue
                    continue;
                }
            }
        }

        self.flush()?;
        Ok(())
    }

    fn handle_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Init(time, cols, rows, _pid, _seq, _text) => {
                self.start_time = Instant::now();
                self.last_event_time = Some(self.start_time);

                if !self.header_written || !self.config.append {
                    self.write_header(cols, rows, time)?;
                    self.header_written = true;
                }

                // Do NOT emit init seq - recording starts from first real output
            }

            Event::Output(_time, data) => {
                let interval = self.calculate_interval();
                self.write_event(interval, "o", &data)?;
            }

            Event::Resize(_time, cols, rows) => {
                let interval = self.calculate_interval();
                let data = format!("{}x{}", cols, rows);
                self.write_event(interval, "r", &data)?;
            }

            Event::Marker(_time, label) => {
                let interval = self.calculate_interval();
                self.write_event(interval, "m", &label)?;
            }

            Event::Input(_time, data) if self.config.capture_input => {
                let interval = self.calculate_interval();
                self.write_event(interval, "i", &data)?;
            }

            Event::Exit(_time, status) => {
                let interval = self.calculate_interval();
                self.write_event_with_number(interval, "x", status)?;
            }

            Event::Snapshot(_, _, _, _) | Event::Input(_, _) => {
                // Ignore snapshots and input if not capturing
            }
        }

        Ok(())
    }

    fn write_header(&mut self, cols: usize, rows: usize, _timestamp: f64) -> Result<()> {
        let mut header = json!({
            "version": 3,
            "term": {
                "cols": cols,
                "rows": rows,
            }
        });

        if let Some(term_type) = &self.config.term_type {
            header["term"]["type"] = json!(term_type);
        }

        if let Some(theme) = &self.config.theme {
            let mut theme_obj = json!({
                "fg": theme.fg,
                "bg": theme.bg,
            });
            if let Some(palette) = &theme.palette {
                theme_obj["palette"] = json!(palette);
            }
            header["term"]["theme"] = theme_obj;
        }

        // Use actual Unix timestamp instead of event time
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        header["timestamp"] = json!(timestamp);

        if let Some(idle_limit) = self.config.idle_time_limit {
            header["idle_time_limit"] = json!(idle_limit);
        }

        if let Some(command) = &self.config.command {
            header["command"] = json!(command);
        }

        if let Some(title) = &self.config.title {
            header["title"] = json!(title);
        }

        if !self.config.capture_env.is_empty() {
            let env_map: serde_json::Map<String, serde_json::Value> = self
                .config
                .capture_env
                .iter()
                .filter_map(|key| std::env::var(key).ok().map(|val| (key.clone(), json!(val))))
                .collect();
            header["env"] = serde_json::Value::Object(env_map);
        }

        writeln!(self.writer, "{}", header)?;
        self.writer.flush()?;
        Ok(())
    }

    fn write_event(&mut self, interval: f64, code: &str, data: &str) -> Result<()> {
        let event = json!([interval, code, data]);
        writeln!(self.writer, "{}", event)?;

        // Flush frequently to avoid data loss on crash
        self.writer.flush()?;
        Ok(())
    }

    fn write_event_with_number(&mut self, interval: f64, code: &str, data: i32) -> Result<()> {
        let event = json!([interval, code, data]);
        writeln!(self.writer, "{}", event)?;

        // Flush frequently to avoid data loss on crash
        self.writer.flush()?;
        Ok(())
    }

    fn calculate_interval(&mut self) -> f64 {
        let now = Instant::now();
        let interval = if let Some(last) = self.last_event_time {
            now.duration_since(last).as_secs_f64()
        } else {
            0.0
        };

        self.last_event_time = Some(now);

        // Apply idle time limit if configured
        if let Some(limit) = self.config.idle_time_limit {
            interval.min(limit)
        } else {
            interval
        }
    }

    fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;

    #[test]
    fn test_header_generation() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join(format!("test_asciicast_{}.cast", uuid::Uuid::new_v4()));

        let config = RecorderConfig {
            output_path: test_file.clone(),
            append: false,
            idle_time_limit: Some(2.0),
            title: Some("Test Recording".to_string()),
            command: Some("bash".to_string()),
            capture_env: vec!["SHELL".to_string()],
            theme: Some(ThemeConfig {
                fg: "#ffffff".to_string(),
                bg: "#000000".to_string(),
                palette: None,
            }),
            term_type: Some("xterm-256color".to_string()),
            capture_input: false,
        };

        let mut recorder = AsciicastV3Recorder::new(config).unwrap();
        recorder.write_header(80, 24, 1234567890.0).unwrap();
        recorder.flush().unwrap();

        let file = File::open(&test_file).unwrap();
        let reader = std::io::BufReader::new(file);
        let first_line = reader.lines().next().unwrap().unwrap();
        let header: serde_json::Value = serde_json::from_str(&first_line).unwrap();

        assert_eq!(header["version"], 3);
        assert_eq!(header["term"]["cols"], 80);
        assert_eq!(header["term"]["rows"], 24);
        assert_eq!(header["term"]["type"], "xterm-256color");
        assert_eq!(header["title"], "Test Recording");
        assert_eq!(header["command"], "bash");
        assert_eq!(header["idle_time_limit"], 2.0);

        std::fs::remove_file(test_file).ok();
    }

    #[test]
    fn test_event_formatting() {
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join(format!("test_events_{}.cast", uuid::Uuid::new_v4()));

        let config = RecorderConfig {
            output_path: test_file.clone(),
            append: false,
            idle_time_limit: None,
            title: None,
            command: None,
            capture_env: vec![],
            theme: None,
            term_type: None,
            capture_input: false,
        };

        let mut recorder = AsciicastV3Recorder::new(config).unwrap();
        recorder.write_header(80, 24, 0.0).unwrap();
        recorder.write_event(0.5, "o", "hello\n").unwrap();
        recorder.write_event(1.0, "r", "100x30").unwrap();
        recorder.write_event(0.1, "m", "checkpoint").unwrap();
        recorder.flush().unwrap();

        let file = File::open(&test_file).unwrap();
        let reader = std::io::BufReader::new(file);
        let lines: Vec<String> = reader.lines().map(|l| l.unwrap()).collect();

        assert_eq!(lines.len(), 4); // header + 3 events

        let event1: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(event1[0], 0.5);
        assert_eq!(event1[1], "o");
        assert_eq!(event1[2], "hello\n");

        let event2: serde_json::Value = serde_json::from_str(&lines[2]).unwrap();
        assert_eq!(event2[0], 1.0);
        assert_eq!(event2[1], "r");
        assert_eq!(event2[2], "100x30");

        let event3: serde_json::Value = serde_json::from_str(&lines[3]).unwrap();
        assert_eq!(event3[0], 0.1);
        assert_eq!(event3[1], "m");
        assert_eq!(event3[2], "checkpoint");

        std::fs::remove_file(test_file).ok();
    }
}

#[cfg(test)]
mod golden_tests;
