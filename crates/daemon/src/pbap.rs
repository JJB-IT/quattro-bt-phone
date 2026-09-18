//! Reading the phonebook and call history over PBAP, through BlueZ's `obexd` (session bus).
//!
//! Strictly read-only: only `Select` and `PullAll` are ever called. Each sync opens its own
//! OBEX session and closes it afterwards, so no link to the phone is held open between syncs.
//! obexd ties a session to the D-Bus connection that created it, which is why everything here
//! runs on the daemon's one long-lived session-bus connection.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, anyhow, bail};
use futures_util::StreamExt;
use zbus::zvariant::{OwnedObjectPath, Value};
use zbus::{Connection, MessageStream};

use crate::dbus::{self, Props};
use crate::vcard::{self, Card};

const SERVICE: &str = "org.bluez.obex";
const CLIENT: &str = "org.bluez.obex.Client1";
const PHONEBOOK: &str = "org.bluez.obex.PhonebookAccess1";
const TRANSFER: &str = "org.bluez.obex.Transfer1";

/// The phone shows "Allow access to contacts?" when the session opens and gives up by itself
/// after about 25 seconds; this leaves room for a slow link on top.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(60);
/// A phonebook with a thousand photos takes a while over Bluetooth.
const TRANSFER_TIMEOUT: Duration = Duration::from_secs(300);
/// The vCard properties the parser reads (obexd's names).
const FIELDS: &[&str] = &["VERSION", "FN", "N", "ORG", "TEL", "PHOTO", "X-IRMC-CALL-DATETIME"];
/// Safety net in case a Transfer1 signal is missed.
const TRANSFER_POLL: Duration = Duration::from_millis(500);

pub struct Pulled {
    /// The phonebook without entry 0, which is always the phone owner's own card.
    pub contacts: Vec<Card>,
    /// `None` when the phone doesn't share call history.
    pub history: Option<Vec<Card>>,
}

#[derive(Debug)]
pub enum Error {
    /// The session couldn't be opened: the user declined, ignored the prompt, or contact
    /// sharing is off on the phone.
    NotAllowed(anyhow::Error),
    Failed(anyhow::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NotAllowed(e) => write!(f, "the phone didn't allow access to contacts: {e:#}"),
            Error::Failed(e) => write!(f, "{e:#}"),
        }
    }
}

/// Pull contacts and call history from the phone at `address`. `approved` is called once the
/// phone has accepted the session, before the (slower) transfers start.
pub async fn pull(conn: &Connection, address: &str, approved: impl FnOnce()) -> Result<Pulled, Error> {
    let session = tokio::time::timeout(APPROVAL_TIMEOUT, create_session(conn, address))
        .await
        .map_err(|_| Error::NotAllowed(anyhow!("nobody answered the prompt on the phone")))??;
    approved();

    let result = pull_books(conn, &session).await.map_err(Error::Failed);
    if let Err(e) = dbus::call(conn, SERVICE, "/org/bluez/obex", CLIENT, "RemoveSession", &(&session,)).await
    {
        tracing::debug!("closing the PBAP session: {e}");
    }
    result
}

async fn create_session(conn: &Connection, address: &str) -> Result<OwnedObjectPath, Error> {
    let args: HashMap<&str, Value> = HashMap::from([("Target", Value::from("PBAP"))]);
    let reply = conn
        .call_method(Some(SERVICE), "/org/bluez/obex", Some(CLIENT), "CreateSession", &(address, args))
        .await
        .map_err(|e| match e {
            zbus::Error::MethodError(name, ..) if name.as_str().ends_with("ServiceUnknown") => {
                Error::Failed(anyhow!("obexd is not running"))
            }
            // obexd reports a refused or ignored prompt as a generic failure.
            e => Error::NotAllowed(e.into()),
        })?;
    reply.body().deserialize().map_err(|e| Error::Failed(e.into()))
}

async fn pull_books(conn: &Connection, session: &OwnedObjectPath) -> anyhow::Result<Pulled> {
    let mut contacts = pull_book(conn, session, "pb").await.context("reading contacts")?;
    if !contacts.is_empty() {
        contacts.remove(0);
    }

    // Combined history first; some phones only offer the separate lists.
    let history = match pull_book(conn, session, "cch").await {
        Ok(h) => Some(h),
        Err(e) => {
            tracing::info!("combined call history unavailable ({e:#}), reading the separate lists");
            let mut all = Vec::new();
            let mut any = false;
            for book in ["ich", "och", "mch"] {
                match pull_book(conn, session, book).await {
                    Ok(h) => {
                        all.extend(h);
                        any = true;
                    }
                    Err(e) => tracing::info!("{book} unavailable: {e:#}"),
                }
            }
            any.then_some(all)
        }
    };
    Ok(Pulled { contacts, history })
}

async fn pull_book(conn: &Connection, session: &OwnedObjectPath, book: &str) -> anyhow::Result<Vec<Card>> {
    dbus::call(conn, SERVICE, session.as_str(), PHONEBOOK, "Select", &("int", book)).await?;

    // Subscribe before starting, so a fast transfer can't finish unseen.
    let rule =
        dbus::signal_rule("org.freedesktop.DBus.Properties", Some("PropertiesChanged"), "/org/bluez/obex")?;
    let mut changes = MessageStream::for_match_rule(rule, conn, Some(16)).await?;

    // An empty target lets obexd pick a private temporary file; it reports the name back.
    // Name the fields: with an empty filter, Android (tested: Galaxy S25 FE) leaves photos out.
    let filters: HashMap<&str, Value> = HashMap::from([("Fields", Value::from(FIELDS.to_vec()))]);
    let reply =
        conn.call_method(Some(SERVICE), session.as_str(), Some(PHONEBOOK), "PullAll", &("", filters)).await?;
    let (transfer, props): (OwnedObjectPath, Props) = reply.body().deserialize()?;
    let file = dbus::string(&props, "Filename");
    anyhow::ensure!(!file.is_empty(), "obexd didn't say where the {book} transfer goes");

    let finished = tokio::time::timeout(TRANSFER_TIMEOUT, async {
        loop {
            match transfer_status(conn, &transfer).await? {
                Some(s) if s == "complete" => return Ok(()),
                Some(s) if s == "error" => bail!("the phone aborted the transfer"),
                Some(_) => {}
                // obexd drops the object right after the transfer ends; the file tells how.
                None if tokio::fs::metadata(&file).await.is_ok_and(|m| m.len() > 0) => return Ok(()),
                None => bail!("the transfer disappeared"),
            }
            let _ = tokio::time::timeout(TRANSFER_POLL, changes.next()).await;
        }
    })
    .await
    .unwrap_or_else(|_| Err(anyhow!("the transfer took too long")));

    let text = match finished {
        Ok(()) => tokio::fs::read(&file).await.with_context(|| format!("reading {file}")),
        Err(e) => Err(e),
    };
    let _ = tokio::fs::remove_file(&file).await;
    let text = text?;
    // vCards are UTF-8 unless a property says otherwise, which the parser handles per property.
    Ok(vcard::parse(&String::from_utf8_lossy(&text)))
}

/// `None` once obexd has removed the transfer object.
async fn transfer_status(conn: &Connection, transfer: &OwnedObjectPath) -> anyhow::Result<Option<String>> {
    let proxy = zbus::fdo::PropertiesProxy::builder(conn)
        .destination(SERVICE)?
        .path(transfer.as_str())?
        .build()
        .await?;
    let iface = zbus::names::InterfaceName::from_static_str(TRANSFER)?;
    match proxy.get(iface, "Status").await {
        Ok(v) => Ok(Some(String::try_from(v)?)),
        Err(zbus::fdo::Error::UnknownObject(_) | zbus::fdo::Error::UnknownMethod(_)) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
