//! The desktop's file chooser, through `org.freedesktop.portal.FileChooser` (session bus).

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, bail};
use futures_util::StreamExt;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, MatchRule, MessageStream};

const SERVICE: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";

/// Let the user pick one audio file. `None` if they cancelled.
pub async fn pick_audio_file(conn: &Connection, title: &str) -> anyhow::Result<Option<PathBuf>> {
    // The portal answers with a Response signal on a request object whose path it derives from
    // our bus name and a token; subscribe before asking so the answer can't be missed.
    let sender = conn.unique_name().context("no unique bus name")?.trim_start_matches(':').replace('.', "_");
    let token = format!("qbp{}", std::process::id());
    let request = format!("{PATH}/request/{sender}/{token}");
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.portal.Request")?
        .member("Response")?
        .path(request.as_str())?
        .build();
    let mut responses = MessageStream::for_match_rule(rule, conn, Some(4)).await?;

    let filters = vec![("Audio", vec![(1u32, "audio/*")])];
    let options: HashMap<&str, Value> = HashMap::from([
        ("handle_token", Value::from(token.as_str())),
        ("modal", Value::from(true)),
        ("filters", Value::from(filters)),
    ]);
    conn.call_method(
        Some(SERVICE),
        PATH,
        Some("org.freedesktop.portal.FileChooser"),
        "OpenFile",
        &("", title, options),
    )
    .await
    .context("opening the file chooser")?;

    let Some(msg) = responses.next().await else { bail!("the file chooser went away") };
    let (response, results): (u32, HashMap<String, OwnedValue>) = msg?.body().deserialize()?;
    // 0 = chosen, 1 = cancelled, 2 = failed.
    match response {
        0 => {}
        1 => return Ok(None),
        _ => bail!("the file chooser failed"),
    }
    let uris: Vec<String> = match results.get("uris") {
        Some(v) => v.try_clone()?.try_into()?,
        None => Vec::new(),
    };
    Ok(uris.first().and_then(|u| file_path(u)))
}

/// `file:///home/me/My%20tone.mp3` → `/home/me/My tone.mp3`.
fn file_path(uri: &str) -> Option<PathBuf> {
    let path = uri.strip_prefix("file://")?;
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(b) = bytes
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok())
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_file_uris() {
        assert_eq!(file_path("file:///home/me/My%20tone.mp3"), Some(PathBuf::from("/home/me/My tone.mp3")));
        assert_eq!(file_path("file:///tmp/caf%C3%A9.ogg"), Some(PathBuf::from("/tmp/café.ogg")));
        assert_eq!(file_path("https://example.com/a.mp3"), None);
    }
}
