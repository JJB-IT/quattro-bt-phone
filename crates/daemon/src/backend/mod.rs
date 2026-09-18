//! A backend owns the phone: it applies commands and publishes [`State`] changes.
//!
//! Two implementations exist: [`real`] (D-Bus: BlueZ + PipeWire Telephony) and [`mock`]
//! (an in-memory phone for UI development). The socket server talks to either through the
//! same [`Handle`].

pub mod mock;
pub mod real;

use qbp_proto::{Command, Message, State};
use tokio::sync::{mpsc, oneshot, watch};

/// What a command produces: an optional data message (contacts, recents…) for the caller.
pub type Outcome = anyhow::Result<Option<Message>>;

pub struct Job {
    pub command: Command,
    pub reply: oneshot::Sender<Outcome>,
}

/// Cheap to clone; given to every socket client.
#[derive(Clone)]
pub struct Handle {
    jobs: mpsc::Sender<Job>,
    state: watch::Receiver<State>,
}

impl Handle {
    pub fn new() -> (Self, mpsc::Receiver<Job>, watch::Sender<State>) {
        let (jobs, rx) = mpsc::channel(32);
        let (state_tx, state) = watch::channel(State::default());
        (Self { jobs, state }, rx, state_tx)
    }

    pub fn state(&self) -> watch::Receiver<State> {
        self.state.clone()
    }

    pub async fn run(&self, command: Command) -> Outcome {
        let (reply, rx) = oneshot::channel();
        self.jobs
            .send(Job { command, reply })
            .await
            .map_err(|_| anyhow::anyhow!("daemon is shutting down"))?;
        rx.await.map_err(|_| anyhow::anyhow!("command was dropped"))?
    }
}

/// Publish `state` only when it actually changed, so subscribers aren't woken for nothing.
pub fn publish(tx: &watch::Sender<State>, state: &State) {
    tx.send_if_modified(|current| {
        if current == state {
            false
        } else {
            *current = state.clone();
            true
        }
    });
}

/// Normalise a number for comparisons: keep digits, a leading `+`, `*` and `#`.
pub fn normalise_number(n: &str) -> String {
    let mut out = String::with_capacity(n.len());
    for (i, c) in n.trim().chars().enumerate() {
        if c.is_ascii_digit() || c == '*' || c == '#' || (c == '+' && i == 0) {
            out.push(c);
        }
    }
    out
}

/// Validate a dial string before it goes to the phone (the AT command is `ATD<number>;`).
pub fn validate_dial(number: &str) -> anyhow::Result<String> {
    let n = normalise_number(number);
    anyhow::ensure!(!n.is_empty() && n.len() <= 32, "invalid number: {number:?}");
    Ok(n)
}

pub fn validate_tones(digits: &str) -> anyhow::Result<String> {
    let d: String = digits.chars().filter(|c| !c.is_whitespace()).collect();
    anyhow::ensure!(
        !d.is_empty() && d.chars().all(|c| c.is_ascii_digit() || matches!(c, '*' | '#' | 'A'..='D')),
        "invalid DTMF digits: {digits:?}"
    );
    Ok(d)
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(normalise_number(" +31 (6) 1234-5678 "), "+31612345678");
        assert_eq!(normalise_number("0800+123"), "0800123");
        assert!(validate_dial("abc").is_err());
        assert_eq!(validate_dial("*#06#").unwrap(), "*#06#");
    }

    #[test]
    fn tones() {
        assert_eq!(validate_tones("12 34#").unwrap(), "1234#");
        assert!(validate_tones("12x").is_err());
        assert!(validate_tones("").is_err());
    }
}
