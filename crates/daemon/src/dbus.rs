//! Small helpers over zbus shared by the BlueZ, Telephony and notification clients.

use std::collections::HashMap;

use futures_util::{Stream, StreamExt};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{Connection, MatchRule, MessageStream};

pub type Props = HashMap<String, OwnedValue>;
pub type Objects = HashMap<OwnedObjectPath, HashMap<String, Props>>;

/// `GetManagedObjects` flattened to plain strings. A missing service yields an empty map,
/// since "WirePlumber isn't exporting telephony right now" is a normal state, not an error.
pub async fn managed_objects(conn: &Connection, service: &str, path: &str) -> zbus::Result<Objects> {
    let proxy =
        zbus::fdo::ObjectManagerProxy::builder(conn).destination(service)?.path(path)?.build().await?;
    match proxy.get_managed_objects().await {
        Ok(objects) => Ok(objects
            .into_iter()
            .map(|(path, ifaces)| {
                (path, ifaces.into_iter().map(|(name, props)| (name.to_string(), props)).collect())
            })
            .collect()),
        Err(zbus::fdo::Error::ServiceUnknown(_)) | Err(zbus::fdo::Error::NameHasNoOwner(_)) => {
            Ok(Objects::new())
        }
        Err(e) => Err(e.into()),
    }
}

pub fn get<T>(props: &Props, key: &str) -> Option<T>
where
    T: TryFrom<OwnedValue>,
{
    props.get(key).and_then(|v| v.try_clone().ok()).and_then(|v| T::try_from(v).ok())
}

pub fn string(props: &Props, key: &str) -> String {
    get::<String>(props, key).unwrap_or_default()
}

pub fn flag(props: &Props, key: &str) -> bool {
    get::<bool>(props, key).unwrap_or(false)
}

/// Call a method and discard its reply body.
pub async fn call(
    conn: &Connection,
    service: &str,
    path: &str,
    iface: &str,
    method: &str,
    body: &(impl serde::Serialize + zbus::zvariant::DynamicType),
) -> zbus::Result<()> {
    conn.call_method(Some(service), path, Some(iface), method, body).await.map(drop)
}

/// A stream that yields `()` whenever anything matching one of `rules` is signalled.
/// Consumers re-read state on every tick instead of interpreting individual signals, which
/// is simple and can't drift, because these object trees are tiny.
pub async fn change_stream(
    conn: &Connection,
    rules: Vec<MatchRule<'static>>,
) -> zbus::Result<impl Stream<Item = ()> + Unpin + use<>> {
    let mut streams = Vec::new();
    for rule in rules {
        streams.push(MessageStream::for_match_rule(rule, conn, Some(64)).await?.map(|_| ()));
    }
    Ok(futures_util::stream::select_all(streams))
}

pub fn signal_rule(
    interface: &'static str,
    member: Option<&'static str>,
    path_namespace: &'static str,
) -> zbus::Result<MatchRule<'static>> {
    let mut b = MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(interface)?;
    if let Some(m) = member {
        b = b.member(m)?;
    }
    Ok(b.path_namespace(path_namespace)?.build())
}

/// Fires when `name` appears on or leaves the bus (daemon restarts).
pub fn owner_changed_rule(name: &'static str) -> zbus::Result<MatchRule<'static>> {
    Ok(MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .sender("org.freedesktop.DBus")?
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .add_arg(name)?
        .build())
}
