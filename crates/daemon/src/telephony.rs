//! `org.pipewire.Telephony` (session bus): PipeWire's HFP hands-free implementation.
//!
//! Object tree (see docs/research.md):
//! `/org/pipewire/Telephony/agN` = one audio gateway (phone), `…/agN/callM` = one call.
//! The `agN` index is not stable across reconnects, so gateways are always matched by address.

use zbus::Connection;
use zbus::zvariant::OwnedObjectPath;

use crate::dbus;

pub const SERVICE: &str = "org.pipewire.Telephony";
const ROOT: &str = "/org/pipewire/Telephony";
const GATEWAY: &str = "org.pipewire.Telephony.AudioGateway1";
const TRANSPORT: &str = "org.pipewire.Telephony.AudioGatewayTransport1";
const CALL: &str = "org.pipewire.Telephony.Call1";

#[derive(Debug, Clone, PartialEq)]
pub struct Gateway {
    pub path: OwnedObjectPath,
    pub address: String,
    /// SCO transport state; `"active"` means call audio is flowing to the computer.
    pub transport: Option<String>,
    pub calls: Vec<RawCall>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawCall {
    pub path: OwnedObjectPath,
    pub number: String,
    /// Network-supplied name (often empty).
    pub name: String,
    pub state: String,
    pub multiparty: bool,
}

pub async fn gateways(conn: &Connection) -> zbus::Result<Vec<Gateway>> {
    let objects = dbus::managed_objects(conn, SERVICE, ROOT).await?;
    let mut gateways: Vec<Gateway> = objects
        .iter()
        .filter_map(|(path, ifaces)| {
            let g = ifaces.get(GATEWAY)?;
            Some(Gateway {
                path: path.clone(),
                address: dbus::string(g, "Address"),
                transport: ifaces.get(TRANSPORT).map(|t| dbus::string(t, "State")),
                calls: Vec::new(),
            })
        })
        .collect();
    for (path, ifaces) in &objects {
        let Some(c) = ifaces.get(CALL) else { continue };
        let Some(g) =
            gateways.iter_mut().find(|g| path.as_str().starts_with(&format!("{}/", g.path.as_str())))
        else {
            continue;
        };
        g.calls.push(RawCall {
            path: path.clone(),
            number: dbus::string(c, "LineIdentification"),
            name: dbus::string(c, "Name"),
            state: dbus::string(c, "State"),
            multiparty: dbus::flag(c, "Multiparty"),
        });
    }
    for g in &mut gateways {
        g.calls.sort_by(|a, b| a.path.as_str().cmp(b.path.as_str()));
    }
    Ok(gateways)
}

pub async fn dial(conn: &Connection, gw: &Gateway, number: &str) -> zbus::Result<()> {
    dbus::call(conn, SERVICE, gw.path.as_str(), GATEWAY, "Dial", &(number,)).await
}

pub async fn send_tones(conn: &Connection, gw: &Gateway, digits: &str) -> zbus::Result<()> {
    dbus::call(conn, SERVICE, gw.path.as_str(), GATEWAY, "SendTones", &(digits,)).await
}

/// AT+CHLD=2: hold the active call and resume the held one (also plain hold/unhold).
pub async fn swap_calls(conn: &Connection, gw: &Gateway) -> zbus::Result<()> {
    dbus::call(conn, SERVICE, gw.path.as_str(), GATEWAY, "SwapCalls", &()).await
}

pub async fn hangup_all(conn: &Connection, gw: &Gateway) -> zbus::Result<()> {
    dbus::call(conn, SERVICE, gw.path.as_str(), GATEWAY, "HangupAll", &()).await
}

/// Answer a waiting call while one is active: hold the active one (AT+CHLD=2).
pub async fn hold_and_answer(conn: &Connection, gw: &Gateway) -> zbus::Result<()> {
    dbus::call(conn, SERVICE, gw.path.as_str(), GATEWAY, "HoldAndAnswer", &()).await
}

pub async fn answer(conn: &Connection, call: &RawCall) -> zbus::Result<()> {
    dbus::call(conn, SERVICE, call.path.as_str(), CALL, "Answer", &()).await
}

pub async fn hangup(conn: &Connection, call: &RawCall) -> zbus::Result<()> {
    dbus::call(conn, SERVICE, call.path.as_str(), CALL, "Hangup", &()).await
}

/// Pull call audio (SCO) from the handset to the computer.
pub async fn activate_audio(conn: &Connection, gw: &Gateway) -> zbus::Result<()> {
    dbus::call(conn, SERVICE, gw.path.as_str(), TRANSPORT, "Activate", &()).await
}

pub fn change_rules() -> zbus::Result<Vec<zbus::MatchRule<'static>>> {
    Ok(vec![
        dbus::signal_rule("org.freedesktop.DBus.Properties", Some("PropertiesChanged"), ROOT)?,
        dbus::signal_rule("org.freedesktop.DBus.ObjectManager", None, ROOT)?,
        dbus::owner_changed_rule(SERVICE)?,
    ])
}
