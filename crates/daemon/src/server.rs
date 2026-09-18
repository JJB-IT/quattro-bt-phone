//! Unix-socket server: newline-delimited JSON, one task per client.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::Context;
use qbp_proto::{Command, Message, Request};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::mpsc;

use crate::backend::Handle;

/// Longest request line we accept; protects against a client streaming garbage.
const MAX_LINE: usize = 64 * 1024;

pub fn bind(path: &Path) -> anyhow::Result<UnixListener> {
    if path.exists() {
        // A live daemon answers; a stale socket from a crash doesn't.
        if std::os::unix::net::UnixStream::connect(path).is_ok() {
            anyhow::bail!("another quattro-bt-phoned is already listening on {}", path.display());
        }
        std::fs::remove_file(path).with_context(|| format!("removing stale {}", path.display()))?;
    }
    let listener = UnixListener::bind(path).with_context(|| format!("binding {}", path.display()))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

pub async fn serve(listener: UnixListener, handle: Handle) -> anyhow::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let handle = handle.clone();
        tokio::spawn(async move {
            if let Err(e) = client(stream, handle).await {
                tracing::debug!("client ended: {e:#}");
            }
        });
    }
}

async fn client(stream: UnixStream, handle: Handle) -> anyhow::Result<()> {
    let (read, mut write) = stream.into_split();
    let (out, mut out_rx) = mpsc::channel::<Message>(64);

    let writer = tokio::spawn(async move {
        while let Some(msg) = out_rx.recv().await {
            let mut line = serde_json::to_vec(&msg)?;
            line.push(b'\n');
            write.write_all(&line).await?;
        }
        anyhow::Ok(())
    });

    let mut lines = BufReader::new(read).lines();
    let mut subscribed = false;
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_LINE {
            out.send(Message::err(None, "request too long")).await?;
            break;
        }
        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                out.send(Message::err(None, format!("bad request: {e}"))).await?;
                continue;
            }
        };
        match req.command {
            Command::Subscribe => {
                out.send(Message::ok(req.id)).await?;
                if !subscribed {
                    subscribed = true;
                    tokio::spawn(forward_state(handle.clone(), out.clone()));
                }
            }
            Command::GetState => {
                let state = handle.state().borrow().clone();
                out.send(Message::State(state)).await?;
                out.send(Message::ok(req.id)).await?;
            }
            command => {
                let name = command_name(&command);
                match handle.run(command).await {
                    Ok(data) => {
                        if let Some(data) = data {
                            out.send(data).await?;
                        }
                        out.send(Message::ok(req.id)).await?;
                    }
                    Err(e) => {
                        tracing::warn!("{name} failed: {e:#}");
                        out.send(Message::err(req.id, format!("{e:#}"))).await?;
                    }
                }
            }
        }
    }
    drop(out);
    writer.await??;
    Ok(())
}

/// Send the state now and after every change, until the client goes away.
async fn forward_state(handle: Handle, out: mpsc::Sender<Message>) {
    let mut rx = handle.state();
    loop {
        let state = rx.borrow_and_update().clone();
        if out.send(Message::State(state)).await.is_err() {
            return;
        }
        if rx.changed().await.is_err() {
            return;
        }
    }
}

fn command_name(c: &Command) -> String {
    serde_json::to_value(c).ok().and_then(|v| v["cmd"].as_str().map(str::to_owned)).unwrap_or_default()
}
