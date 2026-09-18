//! Call audio between the phone and the computer's speakers and microphone.
//!
//! While a call's audio is on the computer, PipeWire shows it as two Bluetooth (SCO) nodes: a
//! source with the other person's voice and a sink that goes to the phone. WirePlumber doesn't
//! link either, so without this nobody hears anything. Two `pw-loopback` bridges do it:
//! phone source → output device, and input device → phone sink.

use std::process::Stdio;

use anyhow::{Context, bail};
use qbp_proto::AudioDevice;
use serde_json::Value;
use tokio::process::{Child, Command};

/// The bridges' Bluetooth side must never fall back to another device when its node goes
/// away: the default microphone would then play straight into the speakers.
const PHONE_SIDE: &str = "{ node.dont-reconnect = true node.dont-fallback = true }";
const DEVICE_SIDE: &str = "{ media.role = Communication }";

#[derive(Default)]
pub struct Router {
    running: Option<Running>,
    /// Routing is wanted but the phone's nodes weren't there yet (they appear shortly after the
    /// transport turns active); the caller retries while this is set.
    pending: bool,
    muted: bool,
}

struct Running {
    address: String,
    devices: Devices,
    to_phone: String,
    remote: Child,
    /// `None` while muted.
    mic: Option<Child>,
}

/// The devices to use (`node.name`); `None` follows the system default.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Devices {
    pub output: Option<String>,
    pub input: Option<String>,
}

impl Router {
    pub fn pending(&self) -> bool {
        self.pending
    }

    /// Bridge the call audio of the phone at `address`, unless that is already running with
    /// these devices.
    pub async fn start(&mut self, address: &str, devices: Devices) {
        if let Some(r) = &mut self.running {
            let alive = |c: &mut Child| matches!(c.try_wait(), Ok(None));
            let healthy = alive(&mut r.remote) && r.mic.as_mut().is_none_or(alive);
            if healthy && r.address == address && r.devices == devices {
                return;
            }
            if !healthy {
                tracing::info!("call audio bridge stopped; restarting it");
            }
            self.stop().await;
        }
        match self.spawn(address, devices).await {
            Ok(running) => {
                self.running = Some(running);
                self.pending = false;
            }
            Err(e) => {
                if !self.pending {
                    tracing::info!("call audio not routed yet: {e:#}");
                }
                self.pending = true;
            }
        }
    }

    pub async fn stop(&mut self) {
        self.pending = false;
        if let Some(mut r) = self.running.take() {
            let _ = r.remote.kill().await;
            if let Some(mut mic) = r.mic.take() {
                let _ = mic.kill().await;
            }
            tracing::info!("call audio bridge stopped");
        }
    }

    /// Muting stops the microphone bridge, so nothing reaches the phone.
    pub async fn set_muted(&mut self, muted: bool) -> anyhow::Result<()> {
        self.muted = muted;
        let Some(r) = &mut self.running else { return Ok(()) };
        match (&mut r.mic, muted) {
            (Some(mic), true) => {
                let _ = mic.kill().await;
                r.mic = None;
            }
            (None, false) => r.mic = Some(mic_bridge(&r.devices, &r.to_phone)?),
            _ => {}
        }
        Ok(())
    }

    async fn spawn(&self, address: &str, devices: Devices) -> anyhow::Result<Running> {
        let nodes = pw_nodes().await?;
        let (from_phone, to_phone) = phone_nodes(&nodes, address)?;
        tracing::info!(%from_phone, %to_phone, ?devices, muted = self.muted, "routing call audio");
        let remote = loopback(
            "quattro-bt-phone-remote",
            (Some(&from_phone), PHONE_SIDE),
            (devices.output.as_deref(), DEVICE_SIDE),
        )?;
        let mic = if self.muted { None } else { Some(mic_bridge(&devices, &to_phone)?) };
        Ok(Running { address: address.to_string(), devices, to_phone, remote, mic })
    }
}

fn mic_bridge(devices: &Devices, to_phone: &str) -> anyhow::Result<Child> {
    loopback("quattro-bt-phone-mic", (devices.input.as_deref(), DEVICE_SIDE), (Some(to_phone), PHONE_SIDE))
}

/// Speakers and microphones for calls: `(outputs, inputs)`, without the phone's own nodes.
pub async fn devices(phone: &str) -> anyhow::Result<(Vec<AudioDevice>, Vec<AudioDevice>)> {
    Ok(list_devices(&pw_nodes().await?, phone))
}

fn list_devices(nodes: &[Value], phone: &str) -> (Vec<AudioDevice>, Vec<AudioDevice>) {
    let (mut outputs, mut inputs) = (Vec::new(), Vec::new());
    for n in nodes {
        let props = &n["info"]["props"];
        let name = props["node.name"].as_str().unwrap_or_default();
        if name.is_empty() || (!phone.is_empty() && is_phone_node(props, name, phone)) {
            continue;
        }
        let description = ["node.description", "node.nick"]
            .iter()
            .find_map(|k| props[*k].as_str().filter(|d| !d.is_empty()))
            .unwrap_or(name)
            .to_string();
        let device = AudioDevice { name: name.to_string(), description };
        match props["media.class"].as_str().unwrap_or_default() {
            "Audio/Sink" => outputs.push(device),
            // Virtual sources are processed microphones, such as Easy Effects'.
            "Audio/Source" | "Audio/Source/Virtual" => inputs.push(device),
            _ => {}
        }
    }
    outputs.sort_by(|a, b| a.description.cmp(&b.description));
    inputs.sort_by(|a, b| a.description.cmp(&b.description));
    (outputs, inputs)
}

fn is_phone_node(props: &Value, name: &str, address: &str) -> bool {
    props["api.bluez5.address"].as_str().is_some_and(|a| a.eq_ignore_ascii_case(address))
        || name.to_ascii_uppercase().contains(&address.replace(':', "_").to_ascii_uppercase())
}

/// One `pw-loopback` from `capture` to `playback`: each a target node (`None`: the default)
/// and stream properties.
fn loopback(
    name: &str,
    capture: (Option<&str>, &str),
    playback: (Option<&str>, &str),
) -> anyhow::Result<Child> {
    let mut cmd = Command::new("pw-loopback");
    cmd.args(["--name", name, "--channels", "1", "--channel-map", "[ MONO ]"]);
    if let Some(node) = capture.0 {
        cmd.args(["--capture", node]);
    }
    if let Some(node) = playback.0 {
        cmd.args(["--playback", node]);
    }
    cmd.arg(format!("--capture-props={}", capture.1))
        .arg(format!("--playback-props={}", playback.1))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("starting pw-loopback")
}

async fn pw_nodes() -> anyhow::Result<Vec<Value>> {
    let out = Command::new("pw-dump").arg("--no-colors").output().await.context("running pw-dump")?;
    anyhow::ensure!(out.status.success(), "pw-dump failed");
    let all: Vec<Value> = serde_json::from_slice(&out.stdout).context("parsing pw-dump")?;
    Ok(all.into_iter().filter(|o| o["type"] == "PipeWire:Interface:Node").collect())
}

/// The phone's call source and sink (by `node.name`) in `pw-dump` output.
fn phone_nodes(nodes: &[Value], address: &str) -> anyhow::Result<(String, String)> {
    let mut source = None;
    let mut sink = None;
    let mut seen = Vec::new();
    for n in nodes {
        let props = &n["info"]["props"];
        let name = props["node.name"].as_str().unwrap_or_default();
        let class = props["media.class"].as_str().unwrap_or_default();
        let ours = is_phone_node(props, name, address);
        // Only the hands-free (SCO) nodes carry the call, not A2DP media.
        let profile = props["api.bluez5.profile"].as_str().unwrap_or_default();
        if !ours {
            continue;
        }
        seen.push(format!("{name} ({class}, {profile})"));
        if profile.contains("a2dp") {
            continue;
        }
        // WirePlumber may wrap a Bluetooth node (`…/Internal`) in a public one; prefer that.
        let slot = if class.starts_with("Audio/Source") {
            &mut source
        } else if class.starts_with("Audio/Sink") {
            &mut sink
        } else {
            continue;
        };
        let internal = class.ends_with("/Internal");
        if slot.as_ref().is_none_or(|(_, was_internal)| *was_internal && !internal) {
            *slot = Some((name.to_string(), internal));
        }
    }
    match (source, sink) {
        (Some((source, _)), Some((sink, _))) => Ok((source, sink)),
        _ if seen.is_empty() => bail!("the phone's call audio nodes aren't in PipeWire"),
        _ => bail!("no call source and sink among the phone's nodes: {}", seen.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn node(name: &str, class: &str, props: Value) -> Value {
        let mut p = json!({ "node.name": name, "media.class": class });
        p.as_object_mut().unwrap().extend(props.as_object().unwrap().clone());
        json!({ "type": "PipeWire:Interface:Node", "info": { "props": p } })
    }

    #[test]
    fn finds_the_phone_call_nodes() {
        let nodes = vec![
            node(
                "bluez_input.40:58:99:57:89:7D",
                "Audio/Source",
                json!({ "api.bluez5.address": "40:58:99:57:89:7D" }),
            ),
            node(
                "bluez_input.00_11_22_33_44_55.2",
                "Audio/Source",
                json!({ "api.bluez5.profile": "a2dp-source" }),
            ),
            node("bluez_input.00_11_22_33_44_55.0", "Audio/Source", json!({})),
            node("bluez_output.00_11_22_33_44_55.1", "Audio/Sink", json!({})),
            node("alsa_output.pci", "Audio/Sink", json!({})),
        ];
        let (from, to) = phone_nodes(&nodes, "00:11:22:33:44:55").unwrap();
        assert_eq!(from, "bluez_input.00_11_22_33_44_55.0");
        assert_eq!(to, "bluez_output.00_11_22_33_44_55.1");
        assert!(phone_nodes(&nodes, "00:11:22:33:44:66").is_err());
    }

    #[test]
    fn lists_devices_without_the_phone() {
        let nodes = vec![
            node("alsa_output.pci", "Audio/Sink", json!({ "node.description": "Built-in Audio" })),
            node("alsa_input.pci", "Audio/Source", json!({ "node.description": "Built-in Mic" })),
            node("bluez_input.00_11_22_33_44_55.0", "Audio/Source", json!({})),
            node("bluez_capture_internal.40:58", "Stream/Input/Audio/Internal", json!({})),
            node("bluez_input.40_58.0", "Audio/Source/Internal", json!({})),
            node("bluez_input.40:58", "Audio/Source", json!({ "node.description": "Headset" })),
        ];
        let (outputs, inputs) = list_devices(&nodes, "00:11:22:33:44:55");
        assert_eq!(
            outputs,
            vec![AudioDevice { name: "alsa_output.pci".into(), description: "Built-in Audio".into() }]
        );
        let names: Vec<_> = inputs.iter().map(|d| d.description.as_str()).collect();
        assert_eq!(names, ["Built-in Mic", "Headset"]);
    }

    #[test]
    fn prefers_the_public_node_over_the_internal_one() {
        let nodes = vec![
            node("bluez_input.00_11_22_33_44_55.0", "Audio/Source/Internal", json!({})),
            node(
                "bluez_input.00:11:22:33:44:55",
                "Audio/Source",
                json!({ "api.bluez5.address": "00:11:22:33:44:55" }),
            ),
            node("bluez_output.00_11_22_33_44_55.1", "Audio/Sink", json!({})),
        ];
        let (from, _) = phone_nodes(&nodes, "00:11:22:33:44:55").unwrap();
        assert_eq!(from, "bluez_input.00:11:22:33:44:55");
    }
}
