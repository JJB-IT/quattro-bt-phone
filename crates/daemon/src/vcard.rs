//! vCard 2.1 and 3.0, as PBAP serves them for the phonebook (`pb`) and call history (`cch`).
//!
//! Only what the app shows is kept: a display name, phone numbers with labels, the photo and,
//! for call history, `X-IRMC-CALL-DATETIME`. Phones are sloppy with vCards, so parsing never
//! fails; unknown or broken properties are skipped.

use chrono::{Local, NaiveDateTime, TimeZone, Utc};
use qbp_proto::{PhoneNumber, RecentKind};

use crate::backend::normalise_number;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Card {
    /// Empty when the card has no name (common in call history for unknown callers).
    pub name: String,
    pub numbers: Vec<PhoneNumber>,
    /// `data:` URL.
    pub photo: Option<String>,
    /// Set on call-history entries.
    pub call: Option<CallStamp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallStamp {
    pub kind: RecentKind,
    /// Unix seconds.
    pub at: i64,
}

/// Parse every `BEGIN:VCARD … END:VCARD` block in `text`.
pub fn parse(text: &str) -> Vec<Card> {
    let mut cards = Vec::new();
    let mut current: Option<Builder> = None;
    for line in unfold(text) {
        let Some(prop) = Property::parse(&line) else { continue };
        match prop.name.as_str() {
            "BEGIN" if prop.value.eq_ignore_ascii_case("VCARD") => current = Some(Builder::default()),
            "END" if prop.value.eq_ignore_ascii_case("VCARD") => {
                if let Some(b) = current.take() {
                    cards.push(b.finish());
                }
            }
            _ => {
                if let Some(b) = current.as_mut() {
                    b.add(prop);
                }
            }
        }
    }
    cards
}

/// Join folded lines into logical lines.
///
/// vCard 3.0 folds by starting the continuation with a space or tab. vCard 2.1 does that for
/// base64 data too, and quoted-printable values end a line with `=` to continue it.
fn unfold(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut soft_break = false;
    for raw in text.lines() {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        match lines.last_mut() {
            Some(last) if soft_break => last.push_str(raw),
            Some(last) if raw.starts_with([' ', '\t']) => last.push_str(&raw[1..]),
            _ if raw.trim().is_empty() => continue,
            _ => lines.push(raw.to_string()),
        }
        let last = lines.last_mut().expect("a line was just pushed or extended");
        soft_break = is_quoted_printable(last) && last.ends_with('=');
        if soft_break {
            last.pop();
        }
    }
    lines
}

fn is_quoted_printable(line: &str) -> bool {
    let head = line.split(':').next().unwrap_or_default();
    head.to_ascii_uppercase().contains("QUOTED-PRINTABLE")
}

struct Property {
    /// Upper case, without a group prefix (`item1.TEL` → `TEL`).
    name: String,
    /// Upper case `TYPE` values, including 2.1's bare parameters (`TEL;CELL:`).
    types: Vec<String>,
    /// Decoded, but still escaped as 3.0 text (`\,` `\;` `\n`).
    value: String,
    /// Raw base64 or binary-in-text value, set for `ENCODING=BASE64|B`.
    base64: bool,
}

impl Property {
    fn parse(line: &str) -> Option<Self> {
        let (head, value) = line.split_once(':')?;
        let mut parts = head.split(';');
        let name = parts.next()?.rsplit('.').next()?.trim().to_ascii_uppercase();
        if name.is_empty() {
            return None;
        }

        let (mut types, mut encoding, mut charset) = (Vec::new(), String::new(), String::new());
        for param in parts {
            match param.split_once('=') {
                Some((k, v)) => match k.trim().to_ascii_uppercase().as_str() {
                    "TYPE" => {
                        types.extend(v.split(',').map(|t| t.trim().trim_matches('"').to_ascii_uppercase()))
                    }
                    "ENCODING" => encoding = v.trim().to_ascii_uppercase(),
                    "CHARSET" => charset = v.trim().to_ascii_uppercase(),
                    _ => {}
                },
                None => {
                    let p = param.trim().to_ascii_uppercase();
                    if matches!(p.as_str(), "QUOTED-PRINTABLE" | "BASE64" | "B") {
                        encoding = p;
                    } else {
                        types.push(p);
                    }
                }
            }
        }

        let base64 = matches!(encoding.as_str(), "BASE64" | "B");
        let value = if encoding == "QUOTED-PRINTABLE" {
            decode_text(&decode_quoted_printable(value), &charset)
        } else {
            value.to_string()
        };
        Some(Self { name, types, value, base64 })
    }

    fn has_type(&self, t: &str) -> bool {
        self.types.iter().any(|x| x == t)
    }
}

fn decode_quoted_printable(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'='
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    out
}

/// UTF-8 unless the card says otherwise. Anything that isn't valid UTF-8 is read as Latin-1,
/// which is what older phones send when they omit the charset.
fn decode_text(bytes: &[u8], charset: &str) -> String {
    let latin1 = || bytes.iter().map(|&b| b as char).collect();
    match charset {
        "ISO-8859-1" | "LATIN1" | "WINDOWS-1252" => latin1(),
        _ => String::from_utf8(bytes.to_vec()).unwrap_or_else(|_| latin1()),
    }
}

/// Split a structured value on unescaped `sep` and unescape each part.
fn split_escaped(s: &str, sep: char) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('n' | 'N') => parts.last_mut().unwrap().push(' '),
                Some(other) => parts.last_mut().unwrap().push(other),
                None => {}
            },
            c if c == sep => parts.push(String::new()),
            c => parts.last_mut().unwrap().push(c),
        }
    }
    parts
}

fn unescape(s: &str) -> String {
    split_escaped(s, '\0').concat()
}

#[derive(Default)]
struct Builder {
    formatted: String,
    structured: String,
    org: String,
    card: Card,
}

impl Builder {
    fn add(&mut self, p: Property) {
        match p.name.as_str() {
            "FN" => self.formatted = unescape(&p.value).trim().to_string(),
            "N" => self.structured = structured_name(&p.value),
            "ORG" => self.org = split_escaped(&p.value, ';').join(" ").trim().to_string(),
            "TEL" => {
                let number = p.value.trim();
                let key = normalise_number(number);
                if !key.is_empty() && !self.card.numbers.iter().any(|n| normalise_number(&n.number) == key) {
                    self.card
                        .numbers
                        .push(PhoneNumber { label: tel_label(&p).into(), number: number.into() });
                }
            }
            "PHOTO" if p.base64 && self.card.photo.is_none() => {
                let data: String = p.value.chars().filter(|c| !c.is_whitespace()).collect();
                if !data.is_empty() {
                    self.card.photo = Some(format!("data:{};base64,{data}", photo_mime(&p)));
                }
            }
            "X-IRMC-CALL-DATETIME" => {
                let kind = if p.has_type("MISSED") {
                    RecentKind::Missed
                } else if p.has_type("DIALED") {
                    RecentKind::Outgoing
                } else if p.has_type("RECEIVED") {
                    RecentKind::Incoming
                } else {
                    return;
                };
                if let Some(at) = parse_datetime(p.value.trim()) {
                    self.card.call = Some(CallStamp { kind, at });
                }
            }
            _ => {}
        }
    }

    fn finish(mut self) -> Card {
        self.card.name = [self.formatted, self.structured, self.org]
            .into_iter()
            .find(|n| !n.is_empty())
            .unwrap_or_default();
        self.card
    }
}

/// `N:Family;Given;Middle;Prefix;Suffix` → "Prefix Given Middle Family Suffix".
fn structured_name(value: &str) -> String {
    let p = split_escaped(value, ';');
    let get = |i: usize| p.get(i).map(|s| s.trim()).unwrap_or_default();
    [get(3), get(1), get(2), get(0), get(4)]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn tel_label(p: &Property) -> &'static str {
    let fax = p.has_type("FAX");
    if p.has_type("CELL") {
        "mobile"
    } else if p.has_type("WORK") {
        if fax { "work fax" } else { "work" }
    } else if p.has_type("HOME") {
        if fax { "home fax" } else { "home" }
    } else if fax {
        "fax"
    } else if p.has_type("PAGER") {
        "pager"
    } else if p.has_type("MAIN") {
        "main"
    } else {
        "phone"
    }
}

fn photo_mime(p: &Property) -> &'static str {
    if p.has_type("PNG") || p.has_type("IMAGE/PNG") {
        "image/png"
    } else if p.has_type("GIF") || p.has_type("IMAGE/GIF") {
        "image/gif"
    } else {
        "image/jpeg"
    }
}

/// `20260918T143700` is the phone's local time; a `Z` suffix or an offset makes it absolute.
fn parse_datetime(s: &str) -> Option<i64> {
    if let Some(utc) = s.strip_suffix('Z') {
        let t = NaiveDateTime::parse_from_str(utc, "%Y%m%dT%H%M%S").ok()?;
        return Some(Utc.from_utc_datetime(&t).timestamp());
    }
    if let Ok(t) = chrono::DateTime::parse_from_str(s, "%Y%m%dT%H%M%S%z") {
        return Some(t.timestamp());
    }
    let t = NaiveDateTime::parse_from_str(s, "%Y%m%dT%H%M%S").ok()?;
    Local.from_local_datetime(&t).earliest().map(|t| t.timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    const PB_21: &str = include_str!("../testdata/pb-2.1.vcf");
    const PB_30: &str = include_str!("../testdata/pb-3.0.vcf");
    const CCH_21: &str = include_str!("../testdata/cch-2.1.vcf");

    fn num(label: &str, number: &str) -> PhoneNumber {
        PhoneNumber { label: label.into(), number: number.into() }
    }

    #[test]
    fn phonebook_2_1() {
        let cards = parse(PB_21);
        assert_eq!(cards.len(), 5);

        // The owner card that PBAP always puts first.
        assert_eq!(cards[0].name, "");
        assert!(cards[0].numbers.is_empty());

        assert_eq!(cards[1].name, "Zoë Jansen");
        assert_eq!(cards[1].numbers, vec![num("mobile", "+31 6 1000 0001"), num("home", "010-5550123")]);
        assert_eq!(cards[1].photo.as_deref(), Some("data:image/jpeg;base64,/9j/4AAQSkZJRgABAQ=="));

        // No FN: built from N. Duplicate numbers in another format are dropped.
        assert_eq!(cards[2].name, "Dr. Pieter van den Berg");
        assert_eq!(
            cards[2].numbers,
            vec![num("work", "+31 20 555 0102"), num("work fax", "+31 20 555 0199")]
        );

        // A quoted-printable value folded with soft line breaks.
        assert_eq!(cards[3].name, "Émile Dubois-Laurent de la Fontaine");
        assert_eq!(cards[3].numbers, vec![num("phone", "+33 1 55 50 01 23")]);

        // Only an organisation.
        assert_eq!(cards[4].name, "Mock Plumbing B.V.");
        assert_eq!(cards[4].numbers, vec![num("main", "0800-5550000")]);
    }

    #[test]
    fn phonebook_3_0() {
        let cards = parse(PB_30);
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].name, "Smith, Jo");
        assert_eq!(cards[0].numbers, vec![num("mobile", "+44 7700 900123"), num("home", "+44 20 7946 0000")]);
        assert_eq!(cards[0].photo.as_deref(), Some("data:image/png;base64,iVBORw0KGgoAAAANSUhEUg=="));
        assert_eq!(cards[1].name, "Grace Hopper");
        assert_eq!(cards[1].numbers, vec![num("work", "+1 555 0100")]);
    }

    #[test]
    fn call_history() {
        let cards = parse(CCH_21);
        assert_eq!(cards.len(), 4);

        let stamp = |c: &Card| c.call.unwrap();
        assert_eq!(stamp(&cards[0]).kind, RecentKind::Missed);
        assert_eq!(stamp(&cards[0]).at, 1789727700); // 2026-09-18 10:35:00 UTC
        assert_eq!(cards[0].name, "Zoë Jansen");

        assert_eq!(stamp(&cards[1]).kind, RecentKind::Outgoing);
        let local = Local.with_ymd_and_hms(2026, 9, 17, 18, 5, 0).unwrap().timestamp();
        assert_eq!(stamp(&cards[1]).at, local);
        assert_eq!(cards[1].name, "");
        assert_eq!(cards[1].numbers, vec![num("phone", "+31201234567")]);

        assert_eq!(stamp(&cards[2]).kind, RecentKind::Incoming);
        assert_eq!(stamp(&cards[2]).at, 1789641300); // 12:35 at +0200

        // A withheld number: no TEL at all.
        assert!(cards[3].numbers.is_empty());
        assert_eq!(stamp(&cards[3]).kind, RecentKind::Missed);
    }

    #[test]
    fn junk_is_ignored() {
        assert!(parse("").is_empty());
        assert!(parse("hello\nEND:VCARD\n").is_empty());
        let cards = parse("BEGIN:VCARD\nno colon here\n:empty name\nFN:Ok\nEND:VCARD");
        assert_eq!(cards[0].name, "Ok");
    }

    #[test]
    fn escapes_and_charsets() {
        assert_eq!(split_escaped(r"a\;b;c\,d\\e", ';'), vec!["a;b", r"c,d\e"]);
        assert_eq!(decode_text(&[0x5a, 0x6f, 0xeb], ""), "Zoë");
        assert_eq!(decode_quoted_printable("=C3=A9t=C3=A9 =3D x=4"), "été = x=4".as_bytes());
    }
}
