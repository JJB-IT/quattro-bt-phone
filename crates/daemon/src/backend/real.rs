//! The real phone: BlueZ on the system bus plus PipeWire Telephony on the session bus.
//!
//! Every D-Bus change triggers a full re-read of both object trees (they are tiny), which is
//! turned into a [`State`]. Call tracking derives what D-Bus doesn't say directly: call
//! direction, when a call became active, and whether a call that disappeared was missed.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow, bail};
use futures_util::StreamExt;
use qbp_proto::*;
use tokio::sync::{mpsc, watch};
use zbus::Connection;

use super::{Job, Outcome, now, publish, validate_dial, validate_tones};
use crate::bluez::{self, BtDevice};
use crate::config::Config;
use crate::notify::{self, Notifier, Urgency};
use crate::telephony::{self, Gateway, RawCall};

/// How long "Allow calls" waits for the phone before giving up.
const PERMISSION_TIMEOUT: Duration = Duration::from_secs(45);
/// Safety net in case a signal is missed.
const POLL: Duration = Duration::from_secs(30);

/// Results of slow operations that run off the main loop.
enum Done {
    Connect(anyhow::Result<()>),
    RequestCalls(anyhow::Result<()>),
}

struct Tracked {
    direction: Direction,
    started_at: Option<i64>,
    number: String,
    name: Option<String>,
    last_state: CallState,
}

pub struct Real {
    system: Connection,
    session: Connection,
    config: Config,
    config_path: PathBuf,
    state: State,
    devices: Vec<BtDevice>,
    gateway: Option<Gateway>,
    tracked: HashMap<String, Tracked>,
    call_log: Vec<RecentCall>,
    notifier: Notifier,
    ring_notification: Option<u32>,
    calls_requested: Option<Instant>,
    calls_denied: bool,
    connecting: bool,
    done: mpsc::Sender<Done>,
}

pub async fn run(
    mut jobs: mpsc::Receiver<Job>,
    state_tx: watch::Sender<State>,
    config: Config,
    config_path: PathBuf,
) -> anyhow::Result<()> {
    let system = Connection::system().await.context("connecting to the system bus")?;
    let session = Connection::session().await.context("connecting to the session bus")?;

    let mut bt_changes = crate::dbus::change_stream(&system, bluez::change_rules()?).await?;
    let mut tel_changes = crate::dbus::change_stream(&session, telephony::change_rules()?).await?;
    let (done, mut done_rx) = mpsc::channel(8);

    let mut real = Real {
        notifier: Notifier::new(session.clone(), config.notifications()),
        state: State { settings: Settings { auto_record: config.auto_record }, ..Default::default() },
        system,
        session,
        config,
        config_path,
        devices: Vec::new(),
        gateway: None,
        tracked: HashMap::new(),
        call_log: Vec::new(),
        ring_notification: None,
        calls_requested: None,
        calls_denied: false,
        connecting: false,
        done,
    };
    real.refresh().await;
    publish(&state_tx, &real.state);

    let mut poll = tokio::time::interval(POLL);
    loop {
        tokio::select! {
            job = jobs.recv() => {
                let Some(job) = job else { return Ok(()) };
                let outcome = real.handle(job.command).await;
                real.refresh().await;
                let _ = job.reply.send(outcome);
            }
            Some(()) = bt_changes.next() => real.refresh().await,
            Some(()) = tel_changes.next() => real.refresh().await,
            Some(done) = done_rx.recv() => {
                real.finish(done);
                real.refresh().await;
            }
            _ = poll.tick() => real.refresh().await,
        }
        publish(&state_tx, &real.state);
    }
}

impl Real {
    // ---------------------------------------------------------------- state

    async fn refresh(&mut self) {
        match bluez::devices(&self.system).await {
            Ok(d) => self.devices = d,
            Err(e) => tracing::warn!("reading BlueZ devices: {e}"),
        }
        let gateways = match telephony::gateways(&self.session).await {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!("reading PipeWire telephony: {e}");
                Vec::new()
            }
        };
        self.auto_select_phone();

        let address = self.config.phone.clone().unwrap_or_default();
        self.gateway =
            gateways.into_iter().find(|g| !address.is_empty() && g.address.eq_ignore_ascii_case(&address));
        let device = self.phone_device().cloned();

        let s = &mut self.state;
        s.devices = self
            .devices
            .iter()
            .filter(|d| d.is_phone() && d.paired)
            .map(|d| Device {
                address: d.address.clone(),
                name: d.name.clone(),
                paired: d.paired,
                connected: d.connected,
            })
            .collect();

        let calls = if self.gateway.is_some() {
            self.calls_requested = None;
            self.calls_denied = false;
            Permission::Granted
        } else if self.calls_requested.is_some_and(|t| t.elapsed() < PERMISSION_TIMEOUT) {
            Permission::Requesting
        } else if self.calls_denied {
            Permission::Denied
        } else {
            Permission::Unknown
        };
        s.phone = Phone {
            name: device.as_ref().map(|d| d.name.clone()).unwrap_or_default(),
            address,
            paired: device.as_ref().is_some_and(|d| d.paired),
            connected: device.as_ref().is_some_and(|d| d.connected),
            calls,
            contacts: s.phone.contacts,
            battery: None,
            signal: None,
            operator: None,
        };
        s.audio.route = match self.gateway.as_ref().and_then(|g| g.transport.as_deref()) {
            Some("active") => AudioRoute::Laptop,
            _ if s.calls.is_empty() => AudioRoute::Laptop,
            _ => AudioRoute::Phone,
        };

        let raw = self.gateway.as_ref().map(|g| g.calls.clone()).unwrap_or_default();
        self.track_calls(raw).await;
    }

    /// With exactly one paired phone and nothing configured, use it: that's the common case.
    fn auto_select_phone(&mut self) {
        if self.config.phone.is_some() {
            return;
        }
        let phones: Vec<_> = self.devices.iter().filter(|d| d.is_phone() && d.paired).collect();
        if let [only] = phones.as_slice() {
            tracing::info!(name = %only.name, "using the only paired phone");
            self.config.phone = Some(only.address.clone());
            self.save_config();
        }
    }

    fn phone_device(&self) -> Option<&BtDevice> {
        let address = self.config.phone.as_deref()?;
        self.devices.iter().find(|d| d.address.eq_ignore_ascii_case(address))
    }

    async fn track_calls(&mut self, raw: Vec<RawCall>) {
        let mut calls = Vec::with_capacity(raw.len());
        let mut newly_ringing = None;
        for rc in &raw {
            let id = rc.path.to_string();
            let Some(state) = CallState::from_ofono(&rc.state) else {
                tracing::debug!(state = %rc.state, "unknown call state");
                continue;
            };
            let t = self.tracked.entry(id.clone()).or_insert_with(|| {
                let direction = if state.is_ringing() { Direction::Incoming } else { Direction::Outgoing };
                if state.is_ringing() {
                    newly_ringing = Some(id.clone());
                }
                tracing::info!(?direction, ?state, "call appeared");
                Tracked {
                    direction,
                    started_at: None,
                    number: rc.number.clone(),
                    name: (!rc.name.is_empty()).then(|| rc.name.clone()),
                    last_state: state,
                }
            });
            if state == CallState::Active && t.started_at.is_none() {
                t.started_at = Some(now());
            }
            if !rc.number.is_empty() {
                t.number = rc.number.clone();
            }
            t.last_state = state;
            calls.push(Call {
                id,
                number: t.number.clone(),
                name: t.name.clone(),
                label: None,
                state,
                direction: t.direction,
                started_at: t.started_at,
                multiparty: rc.multiparty,
            });
        }

        let live: Vec<String> = calls.iter().map(|c| c.id.clone()).collect();
        let gone: Vec<String> = self.tracked.keys().filter(|k| !live.contains(k)).cloned().collect();
        for id in gone {
            if let Some(t) = self.tracked.remove(&id) {
                self.log_ended_call(t).await;
            }
        }

        let ringing = calls.iter().find(|c| c.state.is_ringing()).cloned();
        self.state.calls = calls;
        match (ringing, newly_ringing) {
            (Some(call), Some(_)) => {
                if let Some(old) = self.ring_notification.take() {
                    self.notifier.close(old).await;
                }
                let who = call.name.clone().unwrap_or_else(|| display_number(&call.number));
                self.ring_notification =
                    self.notifier.show("Incoming call", &who, notify::GLYPH_RING, Urgency::Critical).await;
            }
            (None, _) => {
                if let Some(id) = self.ring_notification.take() {
                    self.notifier.close(id).await;
                }
            }
            _ => {}
        }
    }

    async fn log_ended_call(&mut self, t: Tracked) {
        let kind = match (t.direction, t.started_at) {
            (Direction::Outgoing, _) => RecentKind::Outgoing,
            (Direction::Incoming, Some(_)) => RecentKind::Incoming,
            (Direction::Incoming, None) => RecentKind::Missed,
        };
        tracing::info!(?kind, "call ended");
        if kind == RecentKind::Missed {
            let who = t.name.clone().unwrap_or_else(|| display_number(&t.number));
            self.notifier.show("Missed call", &who, notify::GLYPH_MISSED, Urgency::Normal).await;
        }
        self.call_log.insert(
            0,
            RecentCall {
                number: t.number,
                name: t.name,
                kind,
                at: t.started_at.unwrap_or_else(now),
                duration: t.started_at.map(|s| (now() - s).max(0) as u32),
                count: 1,
                recording: None,
            },
        );
    }

    fn finish(&mut self, done: Done) {
        match done {
            Done::Connect(r) => {
                self.connecting = false;
                if let Err(e) = r {
                    tracing::warn!("connect failed: {e:#}");
                }
            }
            Done::RequestCalls(r) => {
                if let Err(e) = r {
                    tracing::warn!("hands-free connection refused: {e:#}");
                    self.calls_requested = None;
                    self.calls_denied = true;
                }
            }
        }
    }

    fn save_config(&self) {
        if let Err(e) = self.config.save(&self.config_path) {
            tracing::warn!("saving config: {e:#}");
        }
    }

    // ------------------------------------------------------------- commands

    fn gateway(&self) -> anyhow::Result<&Gateway> {
        self.gateway.as_ref().ok_or_else(|| match self.state.setup_stage() {
            SetupStage::Ready => anyhow!("phone is not ready"),
            stage => anyhow!("phone is not ready ({})", stage_text(stage)),
        })
    }

    fn find_call(&self, id: &Option<String>, pick: impl Fn(CallState) -> bool) -> anyhow::Result<RawCall> {
        let g = self.gateway()?;
        let found = match id {
            Some(id) => g.calls.iter().find(|c| c.path.as_str() == id),
            None => g.calls.iter().find(|c| CallState::from_ofono(&c.state).is_some_and(&pick)),
        };
        found.cloned().ok_or_else(|| anyhow!("no such call"))
    }

    async fn handle(&mut self, cmd: Command) -> Outcome {
        match cmd {
            Command::Subscribe | Command::GetState => {}

            Command::SelectPhone { address } => {
                let d = self
                    .devices
                    .iter()
                    .find(|d| d.address.eq_ignore_ascii_case(&address))
                    .ok_or_else(|| anyhow!("{address} is not a known Bluetooth device"))?;
                anyhow::ensure!(d.is_phone(), "{} doesn't offer hands-free calling", d.name);
                self.config.phone = Some(d.address.clone());
                self.calls_denied = false;
                self.save_config();
            }
            Command::Connect => {
                let d = self.phone_device().cloned().ok_or_else(|| anyhow!("no phone selected"))?;
                anyhow::ensure!(d.paired, "pair {} in Bluetooth settings first", d.name);
                if !self.connecting {
                    self.connecting = true;
                    let (conn, done) = (self.system.clone(), self.done.clone());
                    tokio::spawn(async move {
                        let r = bluez::connect(&conn, &d.path).await.map_err(Into::into);
                        let _ = done.send(Done::Connect(r)).await;
                    });
                }
            }
            Command::RequestCalls => {
                let d = self.phone_device().cloned().ok_or_else(|| anyhow!("no phone selected"))?;
                anyhow::ensure!(d.connected, "{} is not connected", d.name);
                self.calls_requested = Some(Instant::now());
                self.calls_denied = false;
                let (conn, done) = (self.system.clone(), self.done.clone());
                tokio::spawn(async move {
                    let r =
                        bluez::connect_profile(&conn, &d.path, bluez::HFP_AG_UUID).await.map_err(Into::into);
                    let _ = done.send(Done::RequestCalls(r)).await;
                });
            }
            Command::RequestContacts | Command::Sync => bail!("contact sync is not implemented yet (#8)"),

            Command::Dial { number } => {
                let number = validate_dial(&number)?;
                telephony::dial(&self.session, self.gateway()?, &number).await?;
            }
            Command::Answer { call } => {
                let c = self.find_call(&call, CallState::is_ringing)?;
                let others_active = self.state.calls.iter().any(|x| x.state == CallState::Active);
                if others_active && c.state == "waiting" {
                    telephony::hold_and_answer(&self.session, self.gateway()?).await?;
                } else {
                    telephony::answer(&self.session, &c).await?;
                }
            }
            Command::Decline { call } => {
                let c = self.find_call(&call, CallState::is_ringing)?;
                telephony::hangup(&self.session, &c).await?;
            }
            Command::Hangup { call } => {
                let c = self.find_call(&call, |s| {
                    matches!(
                        s,
                        CallState::Active | CallState::Dialing | CallState::Alerting | CallState::Incoming
                    )
                })?;
                telephony::hangup(&self.session, &c).await?;
            }
            Command::HangupAll => telephony::hangup_all(&self.session, self.gateway()?).await?,
            Command::Tones { digits } => {
                let digits = validate_tones(&digits)?;
                telephony::send_tones(&self.session, self.gateway()?, &digits).await?;
            }
            Command::Hold | Command::Swap => telephony::swap_calls(&self.session, self.gateway()?).await?,
            Command::SetRoute { route: AudioRoute::Laptop } => {
                telephony::activate_audio(&self.session, self.gateway()?).await?
            }
            Command::SetRoute { route: AudioRoute::Phone } => {
                bail!("moving call audio back to the phone is not supported yet (#2)")
            }
            Command::SetMuted { .. } => {
                bail!("mute is not implemented yet: it needs the audio routing from #3")
            }

            Command::GetContacts { .. } => return Ok(Some(Message::Contacts { contacts: Vec::new() })),
            Command::GetRecents { missed_only } => {
                let entries = self
                    .call_log
                    .iter()
                    .filter(|r| !missed_only || r.kind == RecentKind::Missed)
                    .cloned()
                    .collect();
                return Ok(Some(Message::Recents { entries }));
            }
            Command::GetRecordings => return Ok(Some(Message::Recordings { recordings: Vec::new() })),

            Command::SetAutoRecord { enabled } => {
                self.config.auto_record = enabled;
                self.state.settings.auto_record = enabled;
                self.save_config();
            }
            Command::StartRecording | Command::StopRecording { .. } | Command::DeleteRecording { .. } => {
                bail!("call recording is not implemented yet (#12)")
            }
            Command::Simulate { .. } => bail!("simulate only works with --mock"),
        }
        Ok(None)
    }
}

fn stage_text(stage: SetupStage) -> &'static str {
    match stage {
        SetupStage::NoPhone => "no phone selected",
        SetupStage::NotPaired => "not paired",
        SetupStage::NotConnected => "not connected",
        SetupStage::NeedsCalls => "calls not allowed on the phone",
        SetupStage::Ready => "ready",
    }
}

fn display_number(n: &str) -> String {
    if n.is_empty() { "Unknown number".into() } else { n.into() }
}
