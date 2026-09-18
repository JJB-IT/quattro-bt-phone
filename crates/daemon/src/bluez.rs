//! BlueZ (system bus): which phones are paired and connected, and asking them to connect.

use zbus::Connection;
use zbus::zvariant::OwnedObjectPath;

use crate::dbus;

pub const SERVICE: &str = "org.bluez";
const DEVICE: &str = "org.bluez.Device1";

/// Hands-Free Audio Gateway: the phone side of HFP. Its presence marks a device as a phone.
pub const HFP_AG_UUID: &str = "0000111f-0000-1000-8000-00805f9b34fb";

#[derive(Debug, Clone, PartialEq)]
pub struct BtDevice {
    pub path: OwnedObjectPath,
    pub address: String,
    pub name: String,
    pub paired: bool,
    pub connected: bool,
    pub uuids: Vec<String>,
}

impl BtDevice {
    pub fn is_phone(&self) -> bool {
        self.uuids.iter().any(|u| u.eq_ignore_ascii_case(HFP_AG_UUID))
    }
}

pub async fn devices(conn: &Connection) -> zbus::Result<Vec<BtDevice>> {
    let objects = dbus::managed_objects(conn, SERVICE, "/").await?;
    let mut out: Vec<BtDevice> = objects
        .into_iter()
        .filter_map(|(path, ifaces)| {
            let p = ifaces.get(DEVICE)?;
            let alias = dbus::string(p, "Alias");
            Some(BtDevice {
                path,
                address: dbus::string(p, "Address"),
                name: if alias.is_empty() { dbus::string(p, "Name") } else { alias },
                paired: dbus::flag(p, "Paired") || dbus::flag(p, "Bonded"),
                connected: dbus::flag(p, "Connected"),
                uuids: dbus::get::<Vec<String>>(p, "UUIDs").unwrap_or_default(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub async fn connect(conn: &Connection, device: &OwnedObjectPath) -> zbus::Result<()> {
    dbus::call(conn, SERVICE, device.as_str(), DEVICE, "Connect", &()).await
}

/// Connecting the hands-free profile is what makes the phone ask "Allow access to calls?".
pub async fn connect_profile(conn: &Connection, device: &OwnedObjectPath, uuid: &str) -> zbus::Result<()> {
    dbus::call(conn, SERVICE, device.as_str(), DEVICE, "ConnectProfile", &(uuid,)).await
}

pub fn change_rules() -> zbus::Result<Vec<zbus::MatchRule<'static>>> {
    Ok(vec![
        dbus::signal_rule("org.freedesktop.DBus.Properties", Some("PropertiesChanged"), "/org/bluez")?,
        dbus::signal_rule("org.freedesktop.DBus.ObjectManager", None, "/")?,
        dbus::owner_changed_rule(SERVICE)?,
    ])
}
