//! Desktop notifications over `org.freedesktop.Notifications`.
//!
//! Omarchy's notification centre shows no action buttons. A click runs the command in the
//! `omarchy-exec` hint, and `omarchy-glyph` sets the icon (see `omarchy-notification-send`).
//! So a notification click opens our panel, which has Answer/Decline. Other notification
//! servers ignore these hints and still show summary and body.

use std::collections::HashMap;

use zbus::Connection;
use zbus::zvariant::Value;

const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";

pub const GLYPH_RING: char = '\u{f11ab}';
pub const GLYPH_MISSED: char = '\u{f03fa}';

/// Opens the plugin panel through Omarchy's IPC entry point.
pub const OPEN_PANEL: &str = "omarchy-shell jjb.bt-phone open";

#[derive(Clone, Copy)]
pub enum Urgency {
    Normal = 1,
    Critical = 2,
}

pub struct Notifier {
    conn: Connection,
    enabled: bool,
}

impl Notifier {
    pub fn new(conn: Connection, enabled: bool) -> Self {
        Self { conn, enabled }
    }

    /// Returns the notification id (for closing it later), or `None` if disabled or failed.
    pub async fn show(&self, summary: &str, body: &str, glyph: char, urgency: Urgency) -> Option<u32> {
        if !self.enabled {
            return None;
        }
        let glyph = glyph.to_string();
        let mut hints: HashMap<&str, Value> = HashMap::new();
        hints.insert("urgency", Value::U8(urgency as u8));
        hints.insert("omarchy-glyph", Value::from(glyph.as_str()));
        hints.insert("omarchy-exec", Value::from(OPEN_PANEL));
        hints.insert("desktop-entry", Value::from("quattro-bt-phone"));
        let actions: Vec<&str> = vec!["default", "Open"];
        let reply = self
            .conn
            .call_method(
                Some(SERVICE),
                PATH,
                Some(SERVICE),
                "Notify",
                &("Phone", 0u32, "call-start", summary, body, actions, hints, -1i32),
            )
            .await;
        match reply.and_then(|m| m.body().deserialize::<u32>()) {
            Ok(id) => Some(id),
            Err(e) => {
                tracing::warn!("notification failed: {e}");
                None
            }
        }
    }

    pub async fn close(&self, id: u32) {
        let _ = self.conn.call_method(Some(SERVICE), PATH, Some(SERVICE), "CloseNotification", &(id,)).await;
    }
}
