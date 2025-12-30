mod api;
mod cli;
mod command;
mod locale;
mod nbio;
mod pty;
mod recording;
mod session;
mod streaming;

use anyhow::{Context, Result};
use cli::{Cli, CliCommand};
use command::Command;
use recording::asciicast_v3::{AsciicastV3Recorder, RecorderConfig, ThemeConfig};
use session::Session;
use std::net::{SocketAddr, TcpListener};
use streaming::asciinema_server::{AsciinemaServerStreamer, StreamProtocol, StreamerConfig};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

#[tokio::main]
async fn main() -> Result<()> {
    locale::check_utf8_locale()?;
    let cli = Cli::new();

    match &cli.command {
        Some(CliCommand::Record {
            out,
            append,
            idle_time_limit,
            title,
            capture_input,
            term_type,
            theme_fg,
            theme_bg,
            capture_env,
        }) => {
            run_record_mode(
                &cli,
                out.clone(),
                *append,
                *idle_time_limit,
                title.clone(),
                *capture_input,
                term_type.clone(),
                theme_fg.clone(),
                theme_bg.clone(),
                capture_env.clone(),
            )
            .await
        }

        Some(CliCommand::Stream {
            server,
            install_id_path,
            install_id_value,
            title,
            visibility,
            protocol,
            capture_input,
            term_type,
            theme_fg,
            theme_bg,
        }) => {
            run_stream_mode(
                &cli,
                server.clone(),
                install_id_path.clone(),
                install_id_value.clone(),
                title.clone(),
                visibility.clone(),
                protocol.clone(),
                *capture_input,
                term_type.clone(),
                theme_fg.clone(),
                theme_bg.clone(),
            )
            .await
        }

        None => run_normal_mode(&cli).await,
    }
}

async fn run_record_mode(
    cli: &Cli,
    output_path: std::path::PathBuf,
    append: bool,
    idle_time_limit: Option<f64>,
    title: Option<String>,
    capture_input: bool,
    term_type: Option<String>,
    theme_fg: Option<String>,
    theme_bg: Option<String>,
    capture_env: Option<String>,
) -> Result<()> {
    let (input_tx, input_rx) = mpsc::channel(1024);
    let (output_tx, output_rx) = mpsc::channel(1024);
    let (command_tx, command_rx) = mpsc::channel(1024);
    let (clients_tx, clients_rx) = mpsc::channel(1);

    let theme = if let (Some(fg), Some(bg)) = (theme_fg, theme_bg) {
        Some(ThemeConfig {
            fg,
            bg,
            palette: None,
        })
    } else {
        None
    };

    let capture_env_list = capture_env
        .map(|s| s.split(',').map(String::from).collect())
        .unwrap_or_default();

    let command_str = if cli.shell_command.is_empty() {
        None
    } else {
        Some(cli.shell_command.join(" "))
    };

    let recorder_config = RecorderConfig {
        output_path,
        append,
        idle_time_limit,
        title,
        command: command_str,
        capture_env: capture_env_list,
        theme,
        term_type,
        capture_input,
    };

    let mut recorder = AsciicastV3Recorder::new(recorder_config)?;
    let clients_tx_clone = clients_tx.clone();

    // Create a channel to signal when the recorder is subscribed and ready
    let (ready_tx, ready_rx) = oneshot::channel();

    // Create session early so recorder can subscribe before PTY starts
    // PID is set to 0 initially; it's only used for the Init event metadata
    let mut session = build_session(&cli.size, 0);

    let recorder_handle = tokio::spawn(async move {
        recorder.run(&clients_tx_clone, Some(ready_tx)).await
    });

    start_http_api(cli.listen, clients_tx.clone()).await?;
    let api = start_stdio_api(command_tx, clients_tx, cli.subscribe.unwrap_or_default());

    // Handle the recorder's subscription request before starting PTY
    // This ensures the recorder is subscribed and won't miss any events
    let mut clients_rx = clients_rx;
    if let Some(client) = clients_rx.recv().await {
        client.accept(session.subscribe());
    }

    // Wait for recorder to signal it's ready (subscription complete)
    ready_rx.await.context("recorder failed to signal ready")?;

    let (pid, pty) = start_pty(&cli.shell_command, &cli.size, input_rx, output_tx)?;

    // Update session with actual PID
    session.set_pid(pid);

    let exit_status = run_event_loop(
        output_rx,
        input_tx,
        command_rx,
        clients_rx,
        session,
        api,
        pty,
        capture_input,
    )
    .await?;

    recorder_handle.await??;

    std::process::exit(exit_status);
}

async fn run_stream_mode(
    cli: &Cli,
    server_url: String,
    install_id_path: Option<std::path::PathBuf>,
    install_id_value: Option<String>,
    title: Option<String>,
    visibility: Option<String>,
    protocol_str: String,
    capture_input: bool,
    term_type: Option<String>,
    theme_fg: Option<String>,
    theme_bg: Option<String>,
) -> Result<()> {
    let (input_tx, input_rx) = mpsc::channel(1024);
    let (output_tx, output_rx) = mpsc::channel(1024);
    let (command_tx, command_rx) = mpsc::channel(1024);
    let (clients_tx, clients_rx) = mpsc::channel(1);

    let protocol = match protocol_str.as_str() {
        "alis" => StreamProtocol::Alis,
        "v3" => StreamProtocol::AsciicastV3,
        _ => anyhow::bail!("invalid protocol: {}", protocol_str),
    };

    let theme = if let (Some(fg), Some(bg)) = (theme_fg, theme_bg) {
        Some(streaming::alis::Theme {
            fg,
            bg,
            palette: Vec::new(),
        })
    } else {
        None
    };

    let streamer_config = StreamerConfig {
        server_url,
        install_id: install_id_value,
        install_id_path,
        title,
        visibility,
        protocol,
        capture_input,
        theme,
        term_type,
    };

    let mut streamer = AsciinemaServerStreamer::new(streamer_config);
    let clients_tx_clone = clients_tx.clone();

    // Create a channel to signal when the streamer is subscribed and ready
    let (ready_tx, ready_rx) = oneshot::channel();

    // Create session early so streamer can subscribe before PTY starts
    // PID is set to 0 initially; it's only used for the Init event metadata
    let mut session = build_session(&cli.size, 0);

    let streamer_handle = tokio::spawn(async move {
        streamer.run(&clients_tx_clone, Some(ready_tx)).await
    });

    start_http_api(cli.listen, clients_tx.clone()).await?;
    let api = start_stdio_api(command_tx, clients_tx, cli.subscribe.unwrap_or_default());

    // Handle the streamer's subscription request before starting PTY
    // This ensures the streamer is subscribed and won't miss any events
    let mut clients_rx = clients_rx;
    if let Some(client) = clients_rx.recv().await {
        client.accept(session.subscribe());
    }

    // Wait for streamer to signal it's ready (subscription complete)
    ready_rx.await.context("streamer failed to signal ready")?;

    let (pid, pty) = start_pty(&cli.shell_command, &cli.size, input_rx, output_tx)?;

    // Update session with actual PID
    session.set_pid(pid);

    let exit_status = run_event_loop(
        output_rx,
        input_tx,
        command_rx,
        clients_rx,
        session,
        api,
        pty,
        capture_input,
    )
    .await?;

    streamer_handle.await??;

    std::process::exit(exit_status);
}

async fn run_normal_mode(cli: &Cli) -> Result<()> {
    let (input_tx, input_rx) = mpsc::channel(1024);
    let (output_tx, output_rx) = mpsc::channel(1024);
    let (command_tx, command_rx) = mpsc::channel(1024);
    let (clients_tx, clients_rx) = mpsc::channel(1);

    start_http_api(cli.listen, clients_tx.clone()).await?;
    let api = start_stdio_api(command_tx, clients_tx, cli.subscribe.unwrap_or_default());
    let (pid, pty) = start_pty(&cli.shell_command, &cli.size, input_rx, output_tx)?;
    let session = build_session(&cli.size, pid);

    let exit_status = run_event_loop(
        output_rx,
        input_tx,
        command_rx,
        clients_rx,
        session,
        api,
        pty,
        false,
    )
    .await?;

    std::process::exit(exit_status);
}

fn build_session(size: &cli::Size, pid: i32) -> Session {
    Session::new(size.cols(), size.rows(), pid)
}

fn start_stdio_api(
    command_tx: mpsc::Sender<Command>,
    clients_tx: mpsc::Sender<session::Client>,
    sub: api::Subscription,
) -> JoinHandle<Result<()>> {
    tokio::spawn(api::stdio::start(command_tx, clients_tx, sub))
}

fn start_pty(
    command: &[String],
    size: &cli::Size,
    input_rx: mpsc::Receiver<Vec<u8>>,
    output_tx: mpsc::Sender<Vec<u8>>,
) -> Result<(i32, JoinHandle<Result<i32>>)> {
    let command_vec: Vec<String> = if command.is_empty() {
        vec!["bash".to_string()]
    } else {
        command.to_vec()
    };
    eprintln!("launching {:?} in terminal of size {}", command_vec, size);
    let (pid, fut) = pty::spawn(&command_vec, size, input_rx, output_tx)?;

    Ok((pid, tokio::spawn(fut)))
}

async fn start_http_api(
    listen_addr: Option<SocketAddr>,
    clients_tx: mpsc::Sender<session::Client>,
) -> Result<()> {
    if let Some(addr) = listen_addr {
        let listener = TcpListener::bind(addr).context("cannot start HTTP listener")?;
        tokio::spawn(api::http::start(listener, clients_tx).await?);
    }

    Ok(())
}

async fn run_event_loop(
    mut output_rx: mpsc::Receiver<Vec<u8>>,
    input_tx: mpsc::Sender<Vec<u8>>,
    mut command_rx: mpsc::Receiver<Command>,
    mut clients_rx: mpsc::Receiver<session::Client>,
    mut session: Session,
    mut api_handle: JoinHandle<Result<()>>,
    mut pty_handle: JoinHandle<Result<i32>>,
    capture_input: bool,
) -> Result<i32> {
    let mut serving = true;
    let mut exit_status = 0;

    loop {
        tokio::select! {
            result = output_rx.recv() => {
                match result {
                    Some(data) => {
                        session.output(String::from_utf8_lossy(&data).to_string());
                    },

                    None => {
                        eprintln!("process exited, shutting down...");
                        break;
                    }
                }
            }

            command = command_rx.recv() => {
                match command {
                    Some(Command::Input(seqs)) => {
                        let data = command::seqs_to_bytes(&seqs, session.cursor_key_app_mode());

                        // Emit Input event if capturing
                        if capture_input {
                            session.input(String::from_utf8_lossy(&data).to_string());
                        }

                        input_tx.send(data).await?;
                    }

                    Some(Command::Snapshot) => {
                        session.snapshot();
                    }

                    Some(Command::Resize(cols, rows)) => {
                        session.resize(cols, rows);
                    }

                    Some(Command::Marker(label)) => {
                        session.marker(label);
                    }

                    None => {
                        eprintln!("stdin closed, shutting down...");
                        break;
                    }
                }
            }

            client = clients_rx.recv(), if serving => {
                match client {
                    Some(client) => {
                        client.accept(session.subscribe());
                    }

                    None => {
                        serving = false;
                    }
                }
            }

            _ = &mut api_handle => {
                eprintln!("stdin closed, shutting down...");
                break;
            }

            result = &mut pty_handle => {
                match result {
                    Ok(Ok(status)) => {
                        exit_status = status;
                        eprintln!("process exited with status: {}", status);
                        session.exit(status);
                        break;
                    }
                    Ok(Err(e)) => {
                        eprintln!("pty error: {}", e);
                        exit_status = 1;
                        session.exit(1);
                        break;
                    }
                    Err(e) => {
                        eprintln!("pty task error: {}", e);
                        exit_status = 1;
                        session.exit(1);
                        break;
                    }
                }
            }
        }
    }

    // Give events a moment to propagate
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(exit_status)
}
