use crate::api::Subscription;
use anyhow::bail;
use clap::{Parser, Subcommand};
use nix::pty;
use std::{fmt::Display, net::SocketAddr, ops::Deref, path::PathBuf, str::FromStr};

#[derive(Debug, Parser)]
#[clap(version, about)]
#[command(name = "ht")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<CliCommand>,

    /// Terminal size
    #[arg(long, value_name = "COLSxROWS", default_value = Some("120x40"), global = true)]
    pub size: Size,

    /// Command to run inside the terminal
    #[arg(default_value = "bash", global = true)]
    pub shell_command: Vec<String>,

    /// Enable HTTP server
    #[arg(short, long, value_name = "LISTEN_ADDR", default_missing_value = "127.0.0.1:0", num_args = 0..=1, global = true)]
    pub listen: Option<SocketAddr>,

    /// Subscribe to events
    #[arg(long, value_name = "EVENTS", global = true)]
    pub subscribe: Option<Subscription>,
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Record a terminal session to an asciicast v3 file
    Record {
        /// Output file path
        #[arg(short, long, value_name = "PATH")]
        out: PathBuf,

        /// Append to existing recording
        #[arg(long)]
        append: bool,

        /// Limit recorded idle time to max seconds
        #[arg(long, value_name = "SECONDS")]
        idle_time_limit: Option<f64>,

        /// Recording title
        #[arg(long, value_name = "TITLE")]
        title: Option<String>,

        /// Capture input (off by default for privacy)
        #[arg(long)]
        capture_input: bool,

        /// Terminal type (e.g., xterm-256color)
        #[arg(long, value_name = "TYPE")]
        term_type: Option<String>,

        /// Theme: fg color (e.g., #ffffff)
        #[arg(long, value_name = "COLOR")]
        theme_fg: Option<String>,

        /// Theme: bg color (e.g., #000000)
        #[arg(long, value_name = "COLOR")]
        theme_bg: Option<String>,

        /// Environment variables to capture (comma-separated, e.g., SHELL,TERM)
        #[arg(long, value_name = "VARS")]
        capture_env: Option<String>,
    },

    /// Stream a terminal session to an asciinema server
    Stream {
        /// Server base URL (e.g., https://asciinema.org)
        #[arg(short, long, value_name = "URL")]
        server: String,

        /// Path to install-id file
        #[arg(long, value_name = "PATH")]
        install_id_path: Option<PathBuf>,

        /// Install ID value (alternative to --install-id-path)
        #[arg(long, value_name = "UUID")]
        install_id_value: Option<String>,

        /// Stream title
        #[arg(long, value_name = "TITLE")]
        title: Option<String>,

        /// Stream visibility (public, unlisted, private)
        #[arg(long, value_name = "VISIBILITY")]
        visibility: Option<String>,

        /// Protocol to use (alis or v3)
        #[arg(long, value_name = "PROTOCOL", default_value = "alis")]
        protocol: String,

        /// Capture input (off by default for privacy)
        #[arg(long)]
        capture_input: bool,

        /// Terminal type (e.g., xterm-256color)
        #[arg(long, value_name = "TYPE")]
        term_type: Option<String>,

        /// Theme: fg color (e.g., #ffffff)
        #[arg(long, value_name = "COLOR")]
        theme_fg: Option<String>,

        /// Theme: bg color (e.g., #000000)
        #[arg(long, value_name = "COLOR")]
        theme_bg: Option<String>,
    },
}

impl Cli {
    pub fn new() -> Self {
        Cli::parse()
    }
}

#[derive(Debug, Clone)]
pub struct Size(pty::Winsize);

impl Size {
    pub fn cols(&self) -> usize {
        self.0.ws_col as usize
    }

    pub fn rows(&self) -> usize {
        self.0.ws_row as usize
    }
}

impl FromStr for Size {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::prelude::v1::Result<Self, Self::Err> {
        match s.split_once('x') {
            Some((cols, rows)) => {
                let cols: u16 = cols.parse()?;
                let rows: u16 = rows.parse()?;

                let winsize = pty::Winsize {
                    ws_col: cols,
                    ws_row: rows,
                    ws_xpixel: 0,
                    ws_ypixel: 0,
                };

                Ok(Size(winsize))
            }

            None => {
                bail!("invalid size format: {s}");
            }
        }
    }
}

impl Deref for Size {
    type Target = pty::Winsize;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Display for Size {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}x{}", self.0.ws_col, self.0.ws_row)
    }
}
