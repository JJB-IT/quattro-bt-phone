//! Wire protocol between `quattro-bt-phoned` and its clients (the Omarchy plugin and the CLI).
//!
//! Transport: a Unix stream socket at `$XDG_RUNTIME_DIR/quattro-bt-phone.sock`, carrying
//! newline-delimited JSON. Clients send [`Request`]s; the daemon sends [`Message`]s. A client
//! that sends `{"cmd":"subscribe"}` receives a full [`State`] immediately and again after
//! every change. See `docs/protocol.md`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const SOCKET_NAME: &str = "quattro-bt-phone.sock";

/// `$XDG_RUNTIME_DIR/quattro-bt-phone.sock`, falling back to `/run/user/$UID`.
pub fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", current_uid())));
    dir.join(SOCKET_NAME)
}

fn current_uid() -> u32 {
    // Avoids a libc dependency: /proc/self is owned by the current user.
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(1000)
}

// ---------------------------------------------------------------------------
// Client → daemon
// ---------------------------------------------------------------------------

/// One line sent by a client. `id` is echoed back in the matching [`Message::Reply`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(flatten)]
    pub command: Command,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Command {
    /// Receive the current state now and after every change.
    Subscribe,
    /// Receive the current state once.
    GetState,

    /// Use this Bluetooth device as the phone (persisted in the config).
    SelectPhone {
        address: String,
    },
    /// Connect the selected phone over Bluetooth.
    Connect,
    /// Connect the hands-free profile; the phone asks the user to allow calls.
    RequestCalls,
    /// Open a PBAP session; the phone asks the user to allow contacts and call history.
    RequestContacts,

    Dial {
        number: String,
    },
    /// Answer the ringing call (or the given one).
    Answer {
        #[serde(default)]
        call: Option<String>,
    },
    /// Reject a ringing call.
    Decline {
        #[serde(default)]
        call: Option<String>,
    },
    /// Hang up one call, or the active one when `call` is omitted.
    Hangup {
        #[serde(default)]
        call: Option<String>,
    },
    HangupAll,
    /// Send DTMF digits (`0-9 * # A-D`) on the active call.
    Tones {
        digits: String,
    },
    /// Put the active call on hold, or resume the held one.
    Hold,
    /// Swap between an active and a held call.
    Swap,
    SetMuted {
        muted: bool,
    },
    SetRoute {
        route: AudioRoute,
    },
    /// The speakers and microphones calls can use.
    GetAudioDevices,
    /// Use this device for calls (a `node.name` from `get_audio_devices`), or the system
    /// default when `name` is omitted. Applies at once, also during a call.
    SetAudioDevice {
        direction: AudioDirection,
        #[serde(default)]
        name: Option<String>,
    },

    /// Re-pull contacts and call history from the phone.
    Sync,
    GetContacts {
        #[serde(default)]
        query: Option<String>,
    },
    GetRecents {
        #[serde(default)]
        missed_only: bool,
    },
    GetRecordings,

    SetAutoRecord {
        enabled: bool,
    },
    StartRecording,
    StopRecording {
        #[serde(default)]
        discard: bool,
    },
    DeleteRecording {
        id: i64,
    },

    /// `--mock` only: simulate something the phone would do.
    Simulate {
        event: SimEvent,
        #[serde(default)]
        number: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimEvent {
    /// An incoming call rings.
    Ring,
    /// The remote party answers our outgoing call.
    RemoteAnswer,
    /// The remote party hangs up (or stops ringing: a missed call).
    RemoteHangup,
    /// Bluetooth link drops.
    Disconnect,
    /// Start over from "no phone selected".
    ResetSetup,
}

// ---------------------------------------------------------------------------
// Daemon → client
// ---------------------------------------------------------------------------

// Messages are serialised and dropped immediately, so the size of `State` doesn't matter.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    State(State),
    Contacts {
        contacts: Vec<Contact>,
    },
    Recents {
        entries: Vec<RecentCall>,
    },
    Recordings {
        recordings: Vec<Recording>,
    },
    AudioDevices {
        outputs: Vec<AudioDevice>,
        inputs: Vec<AudioDevice>,
    },
    Reply {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<u64>,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

impl Message {
    pub fn ok(id: Option<u64>) -> Self {
        Message::Reply { id, ok: true, error: None }
    }
    pub fn err(id: Option<u64>, error: impl Into<String>) -> Self {
        Message::Reply { id, ok: false, error: Some(error.into()) }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub phone: Phone,
    /// Paired Bluetooth devices that can act as a phone (hands-free audio gateway).
    pub devices: Vec<Device>,
    pub calls: Vec<Call>,
    pub audio: Audio,
    pub recording: Option<ActiveRecording>,
    pub sync: SyncInfo,
    pub settings: Settings,
}

impl State {
    /// Where the user is in the first-run flow; the UI shows a setup screen until `Ready`.
    pub fn setup_stage(&self) -> SetupStage {
        let p = &self.phone;
        if p.address.is_empty() {
            SetupStage::NoPhone
        } else if !p.paired {
            SetupStage::NotPaired
        } else if !p.connected {
            SetupStage::NotConnected
        } else if p.calls != Permission::Granted {
            SetupStage::NeedsCalls
        } else {
            SetupStage::Ready
        }
    }

    /// The call the UI should focus on: ringing first, then active, then anything else.
    pub fn focused_call(&self) -> Option<&Call> {
        use CallState::*;
        self.calls
            .iter()
            .find(|c| matches!(c.state, Incoming | Waiting))
            .or_else(|| self.calls.iter().find(|c| matches!(c.state, Active | Dialing | Alerting)))
            .or_else(|| self.calls.first())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Phone {
    /// Empty when no phone has been selected yet.
    pub name: String,
    pub address: String,
    pub paired: bool,
    pub connected: bool,
    /// Hands-free profile: the phone lets us control calls and carry call audio.
    pub calls: Permission,
    /// PBAP: the phone lets us read contacts and call history.
    pub contacts: Permission,
    #[serde(default)]
    pub battery: Option<u8>,
    #[serde(default)]
    pub signal: Option<u8>,
    #[serde(default)]
    pub operator: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Never requested, or the link is down so we can't tell.
    #[default]
    Unknown,
    /// Waiting for the user to tap "Allow" on the phone.
    Requesting,
    Granted,
    Denied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupStage {
    NoPhone,
    NotPaired,
    NotConnected,
    NeedsCalls,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Device {
    pub address: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Call {
    /// Stable within the daemon's lifetime (the D-Bus object path).
    pub id: String,
    pub number: String,
    /// Resolved from the contact cache, else the network-supplied name.
    #[serde(default)]
    pub name: Option<String>,
    /// Number label from the contact ("mobile", "work"…).
    #[serde(default)]
    pub label: Option<String>,
    pub state: CallState,
    pub direction: Direction,
    /// Unix seconds when the call became active.
    #[serde(default)]
    pub started_at: Option<i64>,
    #[serde(default)]
    pub multiparty: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallState {
    Incoming,
    Waiting,
    Dialing,
    Alerting,
    Active,
    Held,
    Disconnected,
}

impl CallState {
    /// Parse the oFono-style state string used by `org.pipewire.Telephony.Call1`.
    pub fn from_ofono(s: &str) -> Option<Self> {
        Some(match s {
            "incoming" => Self::Incoming,
            "waiting" => Self::Waiting,
            "dialing" => Self::Dialing,
            "alerting" => Self::Alerting,
            "active" => Self::Active,
            "held" => Self::Held,
            "disconnected" => Self::Disconnected,
            _ => return None,
        })
    }

    pub fn is_ringing(self) -> bool {
        matches!(self, Self::Incoming | Self::Waiting)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Incoming,
    Outgoing,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Audio {
    pub route: AudioRoute,
    pub muted: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioRoute {
    /// SCO is open: call audio is on the computer.
    #[default]
    Laptop,
    /// Audio stays on the handset.
    Phone,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActiveRecording {
    pub call: String,
    pub path: String,
    pub started_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncInfo {
    pub status: SyncStatus,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub last_synced: Option<i64>,
    pub contacts: u32,
    pub history: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    #[default]
    Idle,
    Syncing,
    /// The phone is showing an "Allow access to contacts?" prompt.
    AwaitingApproval,
    Error,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub auto_record: bool,
    /// `node.name` of the speakers for calls; `None` = the system default.
    #[serde(default)]
    pub audio_output: Option<String>,
    /// `node.name` of the microphone for calls; `None` = the system default.
    #[serde(default)]
    pub audio_input: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioDirection {
    Output,
    Input,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDevice {
    /// PipeWire `node.name`, stable across restarts.
    pub name: String,
    /// What to show, e.g. "Built-in Audio Analog Stereo".
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Contact {
    pub id: i64,
    pub name: String,
    pub numbers: Vec<PhoneNumber>,
    /// `data:` URL of the vCard photo, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub photo: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhoneNumber {
    pub label: String,
    pub number: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecentCall {
    pub number: String,
    #[serde(default)]
    pub name: Option<String>,
    pub kind: RecentKind,
    /// Unix seconds.
    pub at: i64,
    /// Seconds; unknown for history pulled from the phone.
    #[serde(default)]
    pub duration: Option<u32>,
    /// Consecutive calls to/from the same number that were grouped into this entry.
    pub count: u32,
    #[serde(default)]
    pub recording: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecentKind {
    Incoming,
    Outgoing,
    Missed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    pub id: i64,
    pub path: String,
    pub number: String,
    #[serde(default)]
    pub name: Option<String>,
    pub started_at: i64,
    pub duration: u32,
    pub bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_format() {
        let r: Request = serde_json::from_str(r#"{"id":7,"cmd":"dial","number":"+3161234"}"#).unwrap();
        assert_eq!(r.id, Some(7));
        assert_eq!(r.command, Command::Dial { number: "+3161234".into() });

        let r: Request = serde_json::from_str(r#"{"cmd":"hangup"}"#).unwrap();
        assert_eq!(r.command, Command::Hangup { call: None });

        let s = serde_json::to_string(&Request { id: None, command: Command::Subscribe }).unwrap();
        assert_eq!(s, r#"{"cmd":"subscribe"}"#);
    }

    #[test]
    fn state_message_is_flat() {
        let v = serde_json::to_value(Message::State(State::default())).unwrap();
        assert_eq!(v["type"], "state");
        assert_eq!(v["phone"]["connected"], false);
        assert_eq!(v["audio"]["route"], "laptop");
        assert_eq!(v["sync"]["status"], "idle");
    }

    #[test]
    fn reply_roundtrip() {
        let m = Message::err(Some(3), "no phone");
        let s = serde_json::to_string(&m).unwrap();
        assert_eq!(s, r#"{"type":"reply","id":3,"ok":false,"error":"no phone"}"#);
        assert_eq!(serde_json::from_str::<Message>(&s).unwrap(), m);
    }

    #[test]
    fn ofono_states() {
        assert_eq!(CallState::from_ofono("alerting"), Some(CallState::Alerting));
        assert_eq!(CallState::from_ofono("bogus"), None);
        assert!(CallState::Waiting.is_ringing());
    }

    #[test]
    fn setup_stages() {
        let mut s = State::default();
        assert_eq!(s.setup_stage(), SetupStage::NoPhone);
        s.phone.address = "AA:BB:CC:DD:EE:FF".into();
        assert_eq!(s.setup_stage(), SetupStage::NotPaired);
        s.phone.paired = true;
        assert_eq!(s.setup_stage(), SetupStage::NotConnected);
        s.phone.connected = true;
        assert_eq!(s.setup_stage(), SetupStage::NeedsCalls);
        s.phone.calls = Permission::Granted;
        assert_eq!(s.setup_stage(), SetupStage::Ready);
    }

    #[test]
    fn focused_call_prefers_ringing() {
        let call = |id: &str, state| Call {
            id: id.into(),
            number: "1".into(),
            name: None,
            label: None,
            state,
            direction: Direction::Incoming,
            started_at: None,
            multiparty: false,
        };
        let s = State {
            calls: vec![call("a", CallState::Active), call("b", CallState::Waiting)],
            ..Default::default()
        };
        assert_eq!(s.focused_call().unwrap().id, "b");
    }
}
