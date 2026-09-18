//! In-memory phone for UI development (`quattro-bt-phoned --mock`). Nothing touches D-Bus.
//!
//! Drive it from the CLI: `quattro-bt-phone simulate ring`, `… remote-answer`,
//! `… remote-hangup`, `… disconnect`, `… reset-setup`.

use std::time::Duration;

use qbp_proto::*;
use tokio::sync::{mpsc, watch};

use super::{Job, Outcome, normalise_number, now, publish, validate_dial, validate_tones};

const ADDRESS: &str = "00:11:22:33:44:55";

enum Later {
    Connected,
    CallsGranted,
    ContactsGranted,
    Alerting(String),
    Answered(String),
    SyncDone,
}

struct Mock {
    state: State,
    contacts: Vec<Contact>,
    recents: Vec<RecentCall>,
    recordings: Vec<Recording>,
    next_call: u32,
    later: mpsc::Sender<(Duration, Later)>,
}

pub async fn run(mut jobs: mpsc::Receiver<Job>, state_tx: watch::Sender<State>, auto_record: bool) {
    let (later, mut later_rx) = mpsc::channel::<(Duration, Later)>(16);
    let (due_tx, mut due) = mpsc::channel::<Later>(16);
    tokio::spawn(async move {
        while let Some((after, what)) = later_rx.recv().await {
            let due_tx = due_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(after).await;
                let _ = due_tx.send(what).await;
            });
        }
    });

    let mut m = Mock {
        state: ready_state(auto_record),
        contacts: contacts(),
        recents: recents(),
        recordings: recordings(),
        next_call: 1,
        later,
    };
    publish(&state_tx, &m.state);

    loop {
        tokio::select! {
            job = jobs.recv() => {
                let Some(job) = job else { return };
                let outcome = m.handle(job.command).await;
                let _ = job.reply.send(outcome);
            }
            Some(what) = due.recv() => m.fire(what),
        }
        publish(&state_tx, &m.state);
    }
}

fn ready_state(auto_record: bool) -> State {
    State {
        phone: Phone {
            name: "Galaxy S25 FE (mock)".into(),
            address: ADDRESS.into(),
            paired: true,
            connected: true,
            calls: Permission::Granted,
            contacts: Permission::Granted,
            battery: Some(72),
            signal: Some(3),
            operator: Some("Mock Mobile".into()),
        },
        devices: devices(),
        sync: SyncInfo { last_synced: Some(now() - 600), contacts: 38, history: 11, ..Default::default() },
        settings: Settings { auto_record, ..Default::default() },
        ..Default::default()
    }
}

fn devices() -> Vec<Device> {
    vec![
        Device {
            address: ADDRESS.into(),
            name: "Galaxy S25 FE (mock)".into(),
            paired: true,
            connected: false,
        },
        Device {
            address: "00:11:22:33:44:66".into(),
            name: "Old Pixel (mock)".into(),
            paired: true,
            connected: false,
        },
    ]
}

impl Mock {
    fn after(&self, ms: u64, what: Later) {
        let _ = self.later.try_send((Duration::from_millis(ms), what));
    }

    fn call_mut(&mut self, id: &Option<String>, pick: impl Fn(&Call) -> bool) -> anyhow::Result<&mut Call> {
        let calls = &mut self.state.calls;
        let idx = match id {
            Some(id) => calls.iter().position(|c| &c.id == id),
            None => calls.iter().position(&pick),
        };
        idx.map(|i| &mut calls[i]).ok_or_else(|| anyhow::anyhow!("no such call"))
    }

    fn require_ready(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.state.setup_stage() == SetupStage::Ready, "phone is not ready");
        Ok(())
    }

    fn lookup(&self, number: &str) -> (Option<String>, Option<String>) {
        let n = normalise_number(number);
        for c in &self.contacts {
            for p in &c.numbers {
                if normalise_number(&p.number) == n {
                    return (Some(c.name.clone()), Some(p.label.clone()));
                }
            }
        }
        (None, None)
    }

    fn new_call(&mut self, number: &str, state: CallState, direction: Direction) -> String {
        let id = format!("/mock/call{}", self.next_call);
        self.next_call += 1;
        let (name, label) = self.lookup(number);
        self.state.calls.push(Call {
            id: id.clone(),
            number: number.into(),
            name,
            label,
            state,
            direction,
            started_at: None,
            multiparty: false,
        });
        id
    }

    fn activate(&mut self, id: &str) {
        let auto = self.state.settings.auto_record && self.state.audio.route == AudioRoute::Laptop;
        if let Some(c) = self.state.calls.iter_mut().find(|c| c.id == id) {
            c.state = CallState::Active;
            c.started_at.get_or_insert(now());
        }
        if auto && self.state.recording.is_none() {
            self.start_recording(id);
        }
    }

    fn start_recording(&mut self, call: &str) {
        self.state.recording = Some(ActiveRecording {
            call: call.into(),
            path: format!("/tmp/mock-{}.ogg", now()),
            started_at: now(),
        });
    }

    fn stop_recording(&mut self, discard: bool) {
        if let Some(r) = self.state.recording.take()
            && !discard
            && let Some(c) = self.state.calls.iter().find(|c| c.id == r.call)
        {
            self.recordings.insert(
                0,
                Recording {
                    id: self.recordings.len() as i64 + 100,
                    path: r.path,
                    number: c.number.clone(),
                    name: c.name.clone(),
                    started_at: r.started_at,
                    duration: (now() - r.started_at).max(1) as u32,
                    bytes: 64_000,
                },
            );
        }
    }

    /// Remove a call and log it. `missed` = it rang and nobody answered.
    fn end_call(&mut self, id: &str) {
        let Some(i) = self.state.calls.iter().position(|c| c.id == id) else { return };
        if self.state.recording.as_ref().is_some_and(|r| r.call == id) {
            self.stop_recording(false);
        }
        let c = self.state.calls.remove(i);
        let kind = match (c.direction, c.started_at) {
            (Direction::Outgoing, _) => RecentKind::Outgoing,
            (Direction::Incoming, Some(_)) => RecentKind::Incoming,
            (Direction::Incoming, None) => RecentKind::Missed,
        };
        self.recents.insert(
            0,
            RecentCall {
                number: c.number,
                name: c.name,
                kind,
                at: now(),
                duration: c.started_at.map(|s| (now() - s) as u32),
                count: 1,
                recording: None,
            },
        );
        self.state.sync.history = self.recents.len() as u32;
    }

    fn fire(&mut self, what: Later) {
        match what {
            Later::Connected => self.state.phone.connected = true,
            Later::CallsGranted => self.state.phone.calls = Permission::Granted,
            Later::ContactsGranted => {
                self.state.phone.contacts = Permission::Granted;
                self.state.sync.status = SyncStatus::Idle;
                self.state.sync.last_synced = Some(now());
                self.state.sync.contacts = self.contacts.len() as u32;
            }
            Later::Alerting(id) => {
                if let Some(c) =
                    self.state.calls.iter_mut().find(|c| c.id == id && c.state == CallState::Dialing)
                {
                    c.state = CallState::Alerting;
                }
            }
            Later::Answered(id) => {
                if self.state.calls.iter().any(|c| c.id == id && c.state == CallState::Alerting) {
                    self.activate(&id);
                }
            }
            Later::SyncDone => {
                self.state.sync.status = SyncStatus::Idle;
                self.state.sync.last_synced = Some(now());
            }
        }
    }

    async fn handle(&mut self, cmd: Command) -> Outcome {
        match cmd {
            Command::Subscribe | Command::GetState => {}

            Command::SelectPhone { address } => {
                let d = self
                    .state
                    .devices
                    .iter()
                    .find(|d| d.address.eq_ignore_ascii_case(&address))
                    .ok_or_else(|| anyhow::anyhow!("unknown device {address}"))?
                    .clone();
                self.state.phone =
                    Phone { name: d.name, address: d.address, paired: d.paired, ..Default::default() };
            }
            Command::Connect => {
                anyhow::ensure!(self.state.phone.paired, "pair the phone first");
                self.after(800, Later::Connected);
            }
            Command::RequestCalls => {
                anyhow::ensure!(self.state.phone.connected, "phone is not connected");
                self.state.phone.calls = Permission::Requesting;
                self.after(2500, Later::CallsGranted);
            }
            Command::RequestContacts => {
                anyhow::ensure!(self.state.phone.connected, "phone is not connected");
                self.state.phone.contacts = Permission::Requesting;
                self.state.sync.status = SyncStatus::AwaitingApproval;
                self.after(3500, Later::ContactsGranted);
            }

            Command::Dial { number } => {
                self.require_ready()?;
                let number = validate_dial(&number)?;
                let id = self.new_call(&number, CallState::Dialing, Direction::Outgoing);
                self.after(1000, Later::Alerting(id.clone()));
                self.after(4000, Later::Answered(id));
            }
            Command::Answer { call } => {
                let id = self.call_mut(&call, |c| c.state.is_ringing())?.id.clone();
                // Answering a waiting call puts the active one on hold.
                for c in self.state.calls.iter_mut().filter(|c| c.state == CallState::Active) {
                    c.state = CallState::Held;
                }
                self.activate(&id);
            }
            Command::Decline { call } => {
                let id = self.call_mut(&call, |c| c.state.is_ringing())?.id.clone();
                self.end_call(&id);
            }
            Command::Hangup { call } => {
                let id = self.call_mut(&call, |c| c.state != CallState::Held)?.id.clone();
                self.end_call(&id);
            }
            Command::HangupAll => {
                for id in self.state.calls.iter().map(|c| c.id.clone()).collect::<Vec<_>>() {
                    self.end_call(&id);
                }
            }
            Command::Tones { digits } => {
                validate_tones(&digits)?;
                anyhow::ensure!(
                    self.state.calls.iter().any(|c| c.state == CallState::Active),
                    "no active call"
                );
            }
            Command::Hold | Command::Swap => {
                anyhow::ensure!(!self.state.calls.is_empty(), "no call");
                for c in &mut self.state.calls {
                    c.state = match c.state {
                        CallState::Active => CallState::Held,
                        CallState::Held => CallState::Active,
                        s => s,
                    };
                }
            }
            Command::SetMuted { muted } => self.state.audio.muted = muted,
            Command::GetAudioDevices => {
                let d = |name: &str, description: &str| AudioDevice {
                    name: name.into(),
                    description: description.into(),
                };
                return Ok(Some(Message::AudioDevices {
                    outputs: vec![
                        d("mock_output.speakers", "Laptop speakers (mock)"),
                        d("mock_output.headset", "Headset (mock)"),
                    ],
                    inputs: vec![
                        d("mock_input.mic", "Laptop microphone (mock)"),
                        d("mock_input.headset", "Headset mic (mock)"),
                    ],
                }));
            }
            Command::SetAudioDevice { direction, name } => match direction {
                AudioDirection::Output => self.state.settings.audio_output = name,
                AudioDirection::Input => self.state.settings.audio_input = name,
            },
            Command::SetRoute { route } => {
                self.state.audio.route = route;
                if route == AudioRoute::Phone {
                    self.stop_recording(false);
                }
            }

            Command::Sync => {
                self.state.sync.status = SyncStatus::Syncing;
                self.after(1200, Later::SyncDone);
            }
            Command::GetContacts { query } => {
                let q = query.unwrap_or_default().to_lowercase();
                let qn = normalise_number(&q);
                let contacts = self
                    .contacts
                    .iter()
                    .filter(|c| {
                        q.is_empty()
                            || c.name.to_lowercase().contains(&q)
                            || (qn.len() >= 2
                                && c.numbers.iter().any(|n| normalise_number(&n.number).contains(&qn)))
                    })
                    .cloned()
                    .collect();
                return Ok(Some(Message::Contacts { contacts }));
            }
            Command::GetRecents { missed_only } => {
                let entries = self
                    .recents
                    .iter()
                    .filter(|r| !missed_only || r.kind == RecentKind::Missed)
                    .cloned()
                    .collect();
                return Ok(Some(Message::Recents { entries }));
            }
            Command::GetRecordings => {
                return Ok(Some(Message::Recordings { recordings: self.recordings.clone() }));
            }

            Command::SetAutoRecord { enabled } => self.state.settings.auto_record = enabled,
            Command::StartRecording => {
                anyhow::ensure!(self.state.audio.route == AudioRoute::Laptop, "call audio is on the phone");
                let id = self.call_mut(&None, |c| c.state == CallState::Active)?.id.clone();
                if self.state.recording.is_none() {
                    self.start_recording(&id);
                }
            }
            Command::StopRecording { discard } => self.stop_recording(discard),
            Command::DeleteRecording { id } => self.recordings.retain(|r| r.id != id),

            Command::Simulate { event, number } => match event {
                SimEvent::Ring => {
                    self.require_ready()?;
                    let number = number.unwrap_or_else(|| "+31 6 1000 0022".into());
                    let state =
                        if self.state.calls.is_empty() { CallState::Incoming } else { CallState::Waiting };
                    self.new_call(&number, state, Direction::Incoming);
                }
                SimEvent::RemoteAnswer => {
                    let id = self.call_mut(&None, |c| c.state == CallState::Alerting)?.id.clone();
                    self.activate(&id);
                }
                SimEvent::RemoteHangup => {
                    let id = self.call_mut(&None, |_| true)?.id.clone();
                    self.end_call(&id);
                }
                SimEvent::Disconnect => {
                    self.state.calls.clear();
                    self.state.recording = None;
                    self.state.phone.connected = false;
                    self.state.phone.calls = Permission::Unknown;
                }
                SimEvent::ResetSetup => {
                    let auto = self.state.settings.auto_record;
                    self.state = State {
                        devices: devices(),
                        settings: Settings { auto_record: auto, ..self.state.settings.clone() },
                        ..Default::default()
                    };
                }
            },
        }
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Fake data — every name and number is made up.
// ---------------------------------------------------------------------------

fn contacts() -> Vec<Contact> {
    let raw: &[(&str, &[(&str, &str)])] = &[
        ("Alice Brown", &[("mobile", "+31 6 1000 0001")]),
        ("Andre de Vries", &[("work", "+31 20 555 0102"), ("mobile", "+31 6 1000 0002")]),
        ("Anna Smit", &[("mobile", "+31 6 1000 0003")]),
        ("Ben Kok", &[("mobile", "+31 6 1000 0004")]),
        ("Bram Visser", &[("home", "+31 30 555 0105")]),
        ("Carla Jansen", &[("mobile", "+31 6 1000 0006")]),
        ("Chris Bakker", &[("mobile", "+31 6 1000 0007")]),
        ("Daan Mulder", &[("mobile", "+31 6 1000 0008")]),
        ("Dentist", &[("work", "+31 20 555 0109")]),
        ("Emma Peters", &[("mobile", "+31 6 1000 0011")]),
        ("Eva de Boer", &[("mobile", "+31 6 1000 0010")]),
        ("Finn Hendriks", &[("mobile", "+31 6 1000 0012")]),
        ("Garage Van Dijk", &[("work", "+31 70 555 0113")]),
        ("Hanna Dekker", &[("mobile", "+31 6 1000 0014")]),
        ("Isa Brouwer", &[("mobile", "+31 6 1000 0015")]),
        ("Jan de Groot", &[("mobile", "+31 6 1000 0016"), ("home", "+31 10 555 0116")]),
        ("Julia Vos", &[("mobile", "+31 6 1000 0017")]),
        ("Kees Willems", &[("mobile", "+31 6 1000 0018")]),
        ("Lars Meijer", &[("mobile", "+31 6 1000 0019")]),
        ("Lisa van Leeuwen", &[("mobile", "+31 6 1000 0020")]),
        ("Mila Koster", &[("mobile", "+31 6 1000 0021")]),
        ("Mum", &[("mobile", "+31 6 1000 0022"), ("home", "+31 40 555 0122")]),
        ("Noah Prins", &[("mobile", "+31 6 1000 0023")]),
        ("Olivia Huisman", &[("mobile", "+31 6 1000 0024")]),
        ("Pieter Kuiper", &[("work", "+31 20 555 0125")]),
        ("Pizza Roma", &[("work", "+31 20 555 0126")]),
        ("Quinten Bos", &[("mobile", "+31 6 1000 0027")]),
        ("Roos Maas", &[("mobile", "+31 6 1000 0028")]),
        ("Sam Verhoeven", &[("mobile", "+31 6 1000 0029")]),
        ("Sara Schouten", &[("mobile", "+31 6 1000 0030")]),
        ("Tess van den Berg", &[("mobile", "+31 6 1000 0032")]),
        ("Thijs Jacobs", &[("mobile", "+31 6 1000 0031")]),
        ("Uma Kramer", &[("mobile", "+31 6 1000 0033")]),
        ("Vet Clinic", &[("work", "+31 30 555 0134")]),
        ("Wouter Smits", &[("mobile", "+31 6 1000 0035")]),
        ("Xander Post", &[("mobile", "+31 6 1000 0036")]),
        ("Yara Blom", &[("mobile", "+31 6 1000 0037")]),
        ("Zoë Evers", &[("mobile", "+31 6 1000 0038")]),
    ];
    raw.iter()
        .enumerate()
        .map(|(i, (name, nums))| Contact {
            id: i as i64 + 1,
            name: (*name).into(),
            numbers: nums
                .iter()
                .map(|(label, number)| PhoneNumber { label: (*label).into(), number: (*number).into() })
                .collect(),
            photo: (*name == "Mum").then(|| MUM_PHOTO.into()),
        })
        .collect()
}

/// A 48×48 gradient, so the panel's photo path gets exercised.
const MUM_PHOTO: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAADAAAAAwCAMAAABg3Am1AAAAq1BMVEVBZYpEZopHaIlKaYlNaolRa4lTbIhXbohab4hecYhhcodkc4dfcYdldIdqdodtd4ZxeIZ3e4Z4e4V5fIVbcIh9fYWCf4WGgYSIgYRveIaMg4SNhISPhIOThoOXh4ODgISaiYOciYKgi4KijIKkjYKXiIOpj4GtkIGwkYGrkIG1lIC7loC0k4C9l4C/mH/BmH/Gmn/Im3/JnH/Pnn7ToH7WoX7Zon3do33jpnxuu/NyAAAC0klEQVRIx61W23KqQBDMgtwFBOUiCKISMBpFo8b8/5ed3gUUFS9Jnamy9KF7e3p2dsa3t/8QhHA8ghDyEpjnO6egrMdwDmhBlKoQRXC4BxTABUmSZVlhgR+SJHT4ewxSwhVFVVUNga+u8oACvAi4quoGDRMfXVdVUMRWBsHxgGu6YZq9KkzT0DVKETrkFo/juzgdaMuy+wjbssAx9FYGxQ9UDXDL6juO49JwnD44pqGpA6RFLvMXgKfHA+66nucjPA+cvsVEBpJw4YPvSDLwQ8t2KDoIR4gwoBzHtoZgyFKHv0hIVjTgcbznh1EUs4ii0PeoyNDQFFnscM2EFNUwgR/7AdBJMkEkCTiBPwbDNFSlkRTHEjJ7NvBhFCeT6SxFzKaTJKYijt0zWVJcQ0AzepbjAv9O4VmW51mWptPJOxiuY/WQ1EmClAJW3/UC4AHP5yzyLJ2BEXgukmISpC4RHEDA86MY+I/5fLFELD7nH2DEEZKCBFxUheIFuQsHEAjjZAr8YrlaF0WxWi7AmCZxCAm4QKH4uqa0RHAAgTQDfl1sttvNpgAjS5EUdYFCyeV1MwssoyCCQP65LDZfO8QXGJ85JOCC5TQoTXClBduhGc0gsAJ+fzjswVhBYsZysksTXOUZRWUWaEYQ2O2/j8fv/W6zXoIwKU3QwjLXjEA9+yMQPkDY7g7Hn5/jYbctlrA9iUf+fUJ+l2BeEIyagJTWlyndKJAb00VtujiZdhqmz2X1WVmpxKms85ayNi/uvby44uLi4vLi9NPFXbVGTlujqFojb7RGVxb41ubLz82XtzYfy6lu7+SF9r55QGn9gGbtD6jxRL22Jzq+fqK/HwKQeGnMkL8PsuejUr4aldfDuJrFjWHcMr7Zerg37sW2BfG7hdJcWXq1sbCzHqys81Lsnpeiory0R6/X7oNNzRa7eF7swpPF/vu/DjWJ0D8n3Evgp/EPTsGEVh9MhKgAAAAASUVORK5CYII=";

fn recents() -> Vec<RecentCall> {
    let t = now();
    let h = 3600;
    let r = |name: &str, number: &str, kind, ago: i64, duration: Option<u32>, count, recording| RecentCall {
        number: number.into(),
        name: (!name.is_empty()).then(|| name.into()),
        kind,
        at: t - ago,
        duration,
        count,
        recording,
    };
    use RecentKind::*;
    vec![
        r("Mum", "+31 6 1000 0022", Incoming, h, Some(271), 1, Some(1)),
        r("Andre de Vries", "+31 20 555 0102", Outgoing, 3 * h, Some(52), 2, Some(2)),
        r("", "+31 20 555 0199", Missed, 5 * h, None, 1, None),
        r("Ben Kok", "+31 6 1000 0004", Incoming, 22 * h, Some(725), 1, Some(3)),
        r("Pizza Roma", "+31 20 555 0126", Outgoing, 23 * h, Some(70), 1, None),
        r("Eva de Boer", "+31 6 1000 0010", Missed, 28 * h, None, 3, None),
        r("Dentist", "+31 20 555 0109", Incoming, 50 * h, Some(164), 1, None),
        r("Jan de Groot", "+31 6 1000 0016", Outgoing, 70 * h, Some(1420), 1, None),
        r("Lisa van Leeuwen", "+31 6 1000 0020", Incoming, 100 * h, Some(302), 1, None),
        r("", "+44 20 7946 0000", Missed, 120 * h, None, 1, None),
        r("Garage Van Dijk", "+31 70 555 0113", Outgoing, 150 * h, Some(199), 1, None),
    ]
}

fn recordings() -> Vec<Recording> {
    let t = now();
    vec![
        Recording {
            id: 1,
            path: "/tmp/mock-1.ogg".into(),
            number: "+31 6 1000 0022".into(),
            name: Some("Mum".into()),
            started_at: t - 3600,
            duration: 271,
            bytes: 2_100_000,
        },
        Recording {
            id: 2,
            path: "/tmp/mock-2.ogg".into(),
            number: "+31 20 555 0102".into(),
            name: Some("Andre de Vries".into()),
            started_at: t - 3 * 3600,
            duration: 52,
            bytes: 400_000,
        },
        Recording {
            id: 3,
            path: "/tmp/mock-3.ogg".into(),
            number: "+31 6 1000 0004".into(),
            name: Some("Ben Kok".into()),
            started_at: t - 22 * 3600,
            duration: 725,
            bytes: 5_600_000,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Handle;

    async fn start() -> Handle {
        let (handle, jobs, state) = Handle::new();
        tokio::spawn(run(jobs, state, true));
        handle
    }

    #[tokio::test]
    async fn incoming_call_lifecycle() {
        let h = start().await;
        h.run(Command::Simulate { event: SimEvent::Ring, number: None }).await.unwrap();
        let s = h.state().borrow().clone();
        let call = s.focused_call().unwrap();
        assert_eq!(call.state, CallState::Incoming);
        assert_eq!(call.name.as_deref(), Some("Mum"));

        h.run(Command::Answer { call: None }).await.unwrap();
        let s = h.state().borrow().clone();
        assert_eq!(s.calls[0].state, CallState::Active);
        assert!(s.recording.is_some(), "auto-record starts on answer");

        h.run(Command::Hangup { call: None }).await.unwrap();
        let s = h.state().borrow().clone();
        assert!(s.calls.is_empty());
        assert!(s.recording.is_none());
        let Some(Message::Recents { entries }) =
            h.run(Command::GetRecents { missed_only: false }).await.unwrap()
        else {
            panic!()
        };
        assert_eq!(entries[0].kind, RecentKind::Incoming);
    }

    #[tokio::test]
    async fn unanswered_call_is_missed() {
        let h = start().await;
        h.run(Command::Simulate { event: SimEvent::Ring, number: Some("+44 20 7946 0001".into()) })
            .await
            .unwrap();
        h.run(Command::Simulate { event: SimEvent::RemoteHangup, number: None }).await.unwrap();
        let Some(Message::Recents { entries }) =
            h.run(Command::GetRecents { missed_only: true }).await.unwrap()
        else {
            panic!()
        };
        assert_eq!(entries[0].number, "+44 20 7946 0001");
    }

    #[tokio::test]
    async fn setup_flow() {
        let h = start().await;
        h.run(Command::Simulate { event: SimEvent::ResetSetup, number: None }).await.unwrap();
        assert_eq!(h.state().borrow().setup_stage(), SetupStage::NoPhone);
        assert!(h.run(Command::Dial { number: "123".into() }).await.is_err());
        h.run(Command::SelectPhone { address: ADDRESS.into() }).await.unwrap();
        assert_eq!(h.state().borrow().setup_stage(), SetupStage::NotConnected);
    }

    #[tokio::test]
    async fn contacts_search() {
        let h = start().await;
        let Some(Message::Contacts { contacts }) =
            h.run(Command::GetContacts { query: Some("de ".into()) }).await.unwrap()
        else {
            panic!()
        };
        assert!(contacts.iter().all(|c| c.name.to_lowercase().contains("de ")));
        assert!(!contacts.is_empty());
    }
}
