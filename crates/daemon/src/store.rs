//! SQLite cache of what the phone shares over PBAP, plus our own log of calls the daemon saw.
//!
//! One database per phone, at `$XDG_DATA_HOME/quattro-bt-phone/<address>.sqlite3`, so
//! switching phones never mixes their contacts. The phone stays the source of truth: a sync
//! replaces the cached contacts and history wholesale. Our own call log is kept separately
//! because it knows durations and outcomes the phone's history doesn't carry.

use std::path::{Path, PathBuf};

use anyhow::Context;
use qbp_proto::{Contact, PhoneNumber, RecentCall, RecentKind};
use rusqlite::{Connection, OptionalExtension, params};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::backend::normalise_number;
use crate::config::xdg_dir;
use crate::vcard::Card;

const SCHEMA_VERSION: i32 = 1;
const SCHEMA: &str = "
    CREATE TABLE contacts (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        photo TEXT
    );
    CREATE TABLE numbers (
        contact_id INTEGER NOT NULL REFERENCES contacts(id) ON DELETE CASCADE,
        position INTEGER NOT NULL,
        label TEXT NOT NULL,
        number TEXT NOT NULL,
        match_key TEXT NOT NULL
    );
    CREATE INDEX numbers_match_key ON numbers(match_key);
    -- The phone's call history (PBAP cch).
    CREATE TABLE history (
        number TEXT NOT NULL,
        name TEXT,
        kind TEXT NOT NULL,
        at INTEGER NOT NULL
    );
    -- Calls the daemon tracked itself.
    CREATE TABLE call_log (
        id INTEGER PRIMARY KEY,
        number TEXT NOT NULL,
        name TEXT,
        kind TEXT NOT NULL,
        at INTEGER NOT NULL,
        duration INTEGER,
        recording INTEGER
    );
    CREATE TABLE meta (key TEXT PRIMARY KEY, value) WITHOUT ROWID;
";

/// A call in our log and one in the phone's history are the same call when they are this
/// close together. The phone stamps when the call started ringing; we stamp when it connected.
const SAME_CALL_WINDOW: i64 = 120;

pub struct Store {
    conn: Connection,
}

#[derive(Debug, Default, PartialEq)]
pub struct Counts {
    pub contacts: u32,
    pub history: u32,
    pub last_synced: Option<i64>,
}

impl Store {
    pub fn path_for(address: &str) -> PathBuf {
        let file = address.replace(':', "").to_ascii_uppercase();
        xdg_dir("XDG_DATA_HOME", ".local/share").join("quattro-bt-phone").join(format!("{file}.sqlite3"))
    }

    pub fn open(path: &Path) -> anyhow::Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        Self::init(conn)
    }

    /// For tests, and as a fallback when the data directory isn't writable.
    pub fn in_memory() -> anyhow::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> anyhow::Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        let version: i32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        match version {
            0 => {
                conn.execute_batch(SCHEMA)?;
                conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            }
            SCHEMA_VERSION => {}
            v => anyhow::bail!("contact cache has schema version {v}, newer than this daemon"),
        }
        Ok(Self { conn })
    }

    // ------------------------------------------------------------- sync

    /// Replace every cached contact. Cards without a name or number are skipped: the owner's
    /// own card that PBAP puts first usually looks like that.
    pub fn replace_contacts(&mut self, cards: &[Card]) -> anyhow::Result<u32> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM contacts", [])?;
        let mut count = 0;
        {
            let mut contact = tx.prepare("INSERT INTO contacts (name, photo) VALUES (?1, ?2)")?;
            let mut number = tx.prepare(
                "INSERT INTO numbers (contact_id, position, label, number, match_key)
                VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for card in cards.iter().filter(|c| !c.name.is_empty() && !c.numbers.is_empty()) {
                contact.execute(params![card.name, card.photo])?;
                let id = tx.last_insert_rowid();
                for (i, n) in card.numbers.iter().enumerate() {
                    number.execute(params![id, i, n.label, n.number, match_key(&n.number)])?;
                }
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    /// Replace the phone's call history. Cards without a call timestamp are skipped.
    pub fn replace_history(&mut self, cards: &[Card]) -> anyhow::Result<u32> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM history", [])?;
        let mut count = 0;
        {
            let mut insert =
                tx.prepare("INSERT INTO history (number, name, kind, at) VALUES (?1, ?2, ?3, ?4)")?;
            for card in cards {
                let Some(call) = card.call else { continue };
                let number = card.numbers.first().map(|n| n.number.as_str()).unwrap_or_default();
                let name = (!card.name.is_empty()).then_some(card.name.as_str());
                insert.execute(params![number, name, kind_str(call.kind), call.at])?;
                count += 1;
            }
        }
        tx.commit()?;
        Ok(count)
    }

    pub fn set_last_synced(&self, at: i64) -> anyhow::Result<()> {
        self.conn.execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('last_synced', ?1)", [at])?;
        Ok(())
    }

    pub fn counts(&self) -> anyhow::Result<Counts> {
        let count = |table: &str| -> rusqlite::Result<u32> {
            self.conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        };
        let last_synced = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = 'last_synced'", [], |r| r.get(0))
            .optional()?;
        Ok(Counts { contacts: count("contacts")?, history: count("history")?, last_synced })
    }

    // ------------------------------------------------------------ reads

    /// Contacts sorted by name. `query` matches part of a name, or two or more digits of a number.
    pub fn contacts(&self, query: Option<&str>) -> anyhow::Result<Vec<Contact>> {
        let mut stmt = self.conn.prepare(
            "SELECT c.id, c.name, c.photo, n.label, n.number
            FROM contacts c JOIN numbers n ON n.contact_id = c.id
            ORDER BY c.id, n.position",
        )?;
        let mut rows = stmt.query([])?;
        let mut contacts: Vec<Contact> = Vec::new();
        while let Some(r) = rows.next()? {
            let id: i64 = r.get(0)?;
            let number = PhoneNumber { label: r.get(3)?, number: r.get(4)? };
            match contacts.last_mut() {
                Some(c) if c.id == id => c.numbers.push(number),
                _ => contacts.push(Contact { id, name: r.get(1)?, photo: r.get(2)?, numbers: vec![number] }),
            }
        }

        let q = fold(query.unwrap_or_default().trim());
        let digits = normalise_number(&q);
        contacts.retain(|c| {
            q.is_empty()
                || fold(&c.name).contains(&q)
                || (digits.len() >= 2
                    && c.numbers.iter().any(|n| normalise_number(&n.number).contains(&digits)))
        });
        contacts.sort_by_cached_key(|c| fold(&c.name));
        Ok(contacts)
    }

    /// The contact name and number label for a caller, if the number is in the phonebook.
    pub fn lookup(&self, number: &str) -> anyhow::Result<Option<(String, String)>> {
        let key = match_key(number);
        if key.is_empty() {
            return Ok(None);
        }
        let exact = normalise_number(number);
        let mut stmt = self.conn.prepare(
            "SELECT c.name, n.label, n.number FROM numbers n JOIN contacts c ON c.id = n.contact_id
            WHERE n.match_key = ?1",
        )?;
        let candidates: Vec<(String, String, String)> =
            stmt.query_map([key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        // Prefer an exact match when two contacts share the same trailing digits.
        let best = candidates
            .iter()
            .find(|(_, _, n)| normalise_number(n) == exact)
            .or(candidates.first())
            .map(|(name, label, _)| (name.clone(), label.clone()));
        Ok(best)
    }

    pub fn log_call(&self, call: &RecentCall) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO call_log (number, name, kind, at, duration, recording) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![call.number, call.name, kind_str(call.kind), call.at, call.duration, call.recording],
        )?;
        Ok(())
    }

    /// Our call log merged with the phone's history, newest first. A call both sides know about
    /// is shown once, from our log. Consecutive calls with the same number and kind are grouped.
    pub fn recents(&self, missed_only: bool, limit: usize) -> anyhow::Result<Vec<RecentCall>> {
        let read = |sql: &str| -> anyhow::Result<Vec<RecentCall>> {
            let mut stmt = self.conn.prepare(sql)?;
            let rows = stmt.query_map([], |r| {
                Ok(RecentCall {
                    number: r.get(0)?,
                    name: r.get(1)?,
                    kind: parse_kind(&r.get::<_, String>(2)?),
                    at: r.get(3)?,
                    duration: r.get(4)?,
                    count: 1,
                    recording: r.get(5)?,
                })
            })?;
            Ok(rows.collect::<Result<_, _>>()?)
        };
        let ours = read("SELECT number, name, kind, at, duration, recording FROM call_log")?;
        let phone = read("SELECT number, name, kind, at, NULL, NULL FROM history")?;

        let phone: Vec<RecentCall> = phone
            .into_iter()
            .filter(|p| {
                !ours.iter().any(|o| {
                    o.kind == p.kind
                        && (o.at - p.at).abs() <= SAME_CALL_WINDOW
                        && match_key(&o.number) == match_key(&p.number)
                })
            })
            .collect();
        let mut all: Vec<RecentCall> =
            phone.into_iter().chain(ours).filter(|c| !missed_only || c.kind == RecentKind::Missed).collect();
        all.sort_by_key(|c| std::cmp::Reverse(c.at));

        let mut grouped: Vec<RecentCall> = Vec::new();
        for mut call in all {
            let full = grouped.len() == limit;
            match grouped.last_mut() {
                Some(g) if g.kind == call.kind && match_key(&g.number) == match_key(&call.number) => {
                    g.count += 1;
                    g.recording = g.recording.or(call.recording);
                }
                _ if full => break,
                _ => {
                    if let Some((name, _)) = self.lookup(&call.number)? {
                        call.name = Some(name);
                    }
                    grouped.push(call);
                }
            }
        }
        Ok(grouped)
    }
}

/// Lower case without accents, for sorting and searching: "Émile" sorts with "e" and
/// "zoe" finds "Zoë".
fn fold(s: &str) -> String {
    s.nfd().filter(|c| !is_combining_mark(*c)).flat_map(char::to_lowercase).collect()
}

/// The part of a number that identifies it regardless of how it was written: the last nine
/// digits, so `+31 6 1234 5678` matches `06-12345678`. Short numbers must match exactly.
fn match_key(number: &str) -> String {
    let n = normalise_number(number);
    let digits: String = n.chars().filter(char::is_ascii_digit).collect();
    if n.contains(['*', '#']) || digits.len() < 9 {
        n.trim_start_matches('+').to_string()
    } else {
        digits[digits.len() - 9..].to_string()
    }
}

fn kind_str(kind: RecentKind) -> &'static str {
    match kind {
        RecentKind::Incoming => "incoming",
        RecentKind::Outgoing => "outgoing",
        RecentKind::Missed => "missed",
    }
}

fn parse_kind(s: &str) -> RecentKind {
    match s {
        "outgoing" => RecentKind::Outgoing,
        "missed" => RecentKind::Missed,
        _ => RecentKind::Incoming,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vcard;

    fn store() -> Store {
        let mut s = Store::in_memory().unwrap();
        s.replace_contacts(&vcard::parse(include_str!("../testdata/pb-2.1.vcf"))).unwrap();
        s
    }

    fn logged(number: &str, kind: RecentKind, at: i64) -> RecentCall {
        RecentCall {
            number: number.into(),
            name: None,
            kind,
            at,
            duration: Some(42),
            count: 1,
            recording: None,
        }
    }

    #[test]
    fn match_keys() {
        assert_eq!(match_key("+31 6 1000 0001"), match_key("06-10000001"));
        assert_ne!(match_key("+31 6 1000 0001"), match_key("+31 6 1000 0002"));
        assert_eq!(match_key("112"), "112");
        assert_ne!(match_key("112"), match_key("1112"));
        assert_eq!(match_key("*#06#"), "*#06#");
        assert_eq!(match_key(""), "");
    }

    #[test]
    fn contacts_and_search() {
        let s = store();
        let all = s.contacts(None).unwrap();
        // The empty owner card is skipped; the rest are sorted by name.
        let names: Vec<_> = all.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Dr. Pieter van den Berg",
                "Émile Dubois-Laurent de la Fontaine",
                "Mock Plumbing B.V.",
                "Zoë Jansen"
            ]
        );
        assert_eq!(all[0].numbers.len(), 2);
        assert!(all[3].photo.is_some());

        let names = |q| s.contacts(Some(q)).unwrap().into_iter().map(|c| c.name).collect::<Vec<_>>();
        assert_eq!(names("zoë"), ["Zoë Jansen"]);
        assert_eq!(names("ZOE"), ["Zoë Jansen"]);
        assert_eq!(names("555 010"), ["Dr. Pieter van den Berg"]);
        assert_eq!(names("0"), Vec::<String>::new(), "one digit is not a number search");
    }

    #[test]
    fn resync_replaces_everything() {
        let mut s = store();
        assert_eq!(s.counts().unwrap().contacts, 4);
        s.replace_contacts(&vcard::parse(include_str!("../testdata/pb-3.0.vcf"))).unwrap();
        assert_eq!(s.counts().unwrap().contacts, 2);
        assert_eq!(s.lookup("06 1000 0001").unwrap(), None);
        let rows: i64 = s.conn.query_row("SELECT count(*) FROM numbers", [], |r| r.get(0)).unwrap();
        assert_eq!(rows, 3, "numbers of replaced contacts are gone");
    }

    #[test]
    fn caller_lookup() {
        let s = store();
        assert_eq!(s.lookup("0610000001").unwrap(), Some(("Zoë Jansen".into(), "mobile".into())));
        assert_eq!(
            s.lookup("+31205550199").unwrap(),
            Some(("Dr. Pieter van den Berg".into(), "work fax".into()))
        );
        assert_eq!(s.lookup("+31 6 9999 9999").unwrap(), None);
        assert_eq!(s.lookup("").unwrap(), None);
    }

    #[test]
    fn recents_merge_and_group() {
        let mut s = store();
        s.replace_history(&vcard::parse(include_str!("../testdata/cch-2.1.vcf"))).unwrap();
        s.set_last_synced(1_789_730_000).unwrap();
        assert_eq!(s.counts().unwrap(), Counts { contacts: 4, history: 4, last_synced: Some(1_789_730_000) });

        // The phone's missed call at 10:35 UTC, also seen by us a minute later.
        s.log_call(&RecentCall {
            duration: None,
            ..logged("06 1000 0001", RecentKind::Missed, 1_789_727_760)
        })
        .unwrap();
        // Two answered calls from the same number after it.
        s.log_call(&logged("+31610000001", RecentKind::Incoming, 1_789_728_000)).unwrap();
        s.log_call(&logged("+31610000001", RecentKind::Incoming, 1_789_729_000)).unwrap();

        let r = s.recents(false, 50).unwrap();
        assert_eq!(r.len(), 5);
        assert_eq!((r[0].kind, r[0].count, r[0].duration), (RecentKind::Incoming, 2, Some(42)));
        assert_eq!(r[0].name.as_deref(), Some("Zoë Jansen"));
        assert_eq!((r[1].kind, r[1].at), (RecentKind::Missed, 1_789_727_760), "our copy wins");
        assert_eq!(r[2].name.as_deref(), None, "unknown number keeps no name");
        assert_eq!(r[3].name.as_deref(), Some("Grace Hopper"), "the phone's name is kept");

        let missed = s.recents(true, 50).unwrap();
        assert_eq!(missed.len(), 2);
        assert!(missed.iter().all(|c| c.kind == RecentKind::Missed));
        assert_eq!(s.recents(false, 2).unwrap().len(), 2);
    }

    #[test]
    fn database_file_reopens() {
        let dir = std::env::temp_dir().join(format!("qbp-store-{}", std::process::id()));
        let path = dir.join("AABBCCDDEEFF.sqlite3");
        {
            let mut s = Store::open(&path).unwrap();
            s.replace_contacts(&vcard::parse(include_str!("../testdata/pb-3.0.vcf"))).unwrap();
        }
        assert_eq!(Store::open(&path).unwrap().counts().unwrap().contacts, 2);
        std::fs::remove_dir_all(dir).unwrap();
        assert!(Store::path_for("aa:bb:cc:dd:ee:ff").ends_with("quattro-bt-phone/AABBCCDDEEFF.sqlite3"));
    }
}
