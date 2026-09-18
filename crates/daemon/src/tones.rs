//! Sounds the daemon plays itself: keypad feedback (a clicked key's DTMF tone, or a soft tick
//! for a typed one) and the ringing tone while an outgoing call rings.
//!
//! The shell can't play sound, so the daemon does, with `pw-play` on the call speakers. The
//! sounds are synthesised once into `$XDG_RUNTIME_DIR/quattro-bt-phone/`.

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::Context;
use qbp_proto::RingbackStyle;
use tokio::process::Command;
use tokio::task::JoinHandle;

const RATE: u32 = 16_000;

/// Play the sound for `key` (`0-9 * #`). Returns once playback has started.
pub fn play(key: &str, soft: bool, output: Option<&str>) -> anyhow::Result<()> {
    let name = if soft { "tick".to_string() } else { file_name(key)? };
    let path = ensure(&name, || if soft { tick() } else { dtmf(key) })?;
    let mut cmd = Command::new("pw-play");
    cmd.args(["--media-role", "Notification"]);
    if let Some(target) = output {
        cmd.args(["--target", target]);
    }
    let mut child = cmd
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("starting pw-play")?;
    tokio::spawn(async move {
        let _ = child.wait().await;
    });
    Ok(())
}

/// The ringing tone for an outgoing call, looped until dropped or stopped.
#[derive(Default)]
pub struct Ringback {
    playing: Option<JoinHandle<()>>,
}

impl Ringback {
    /// Play `style`; `file` is the user's own tone for `Custom`.
    pub fn start(&mut self, style: RingbackStyle, file: Option<&str>, output: Option<&str>) {
        if style == RingbackStyle::Off {
            return self.stop();
        }
        if self.playing.as_ref().is_some_and(|h| !h.is_finished()) {
            return;
        }
        let path = match ringback_path(style, file) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("ringing tone: {e:#}");
                return;
            }
        };
        let output = output.map(str::to_string);
        self.playing = Some(tokio::spawn(async move {
            loop {
                let mut cmd = Command::new("pw-play");
                cmd.args(["--media-role", "Communication"]);
                if let Some(target) = &output {
                    cmd.args(["--target", target]);
                }
                let child = cmd
                    .arg(&path)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .kill_on_drop(true)
                    .spawn();
                // Aborting the task drops the child, which stops the sound at once.
                let Ok(mut child) = child else { return };
                if !child.wait().await.is_ok_and(|s| s.success()) {
                    return;
                }
            }
        }));
    }

    pub fn stop(&mut self) {
        if let Some(h) = self.playing.take() {
            h.abort();
        }
    }
}

impl Drop for Ringback {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Where the user's own ringing tones go: next to the config.
pub fn ringtones_dir() -> PathBuf {
    let config = crate::config::Config::default_path();
    config.parent().map(Path::to_path_buf).unwrap_or_default().join("ringtones")
}

/// The audio files in the ringtones folder (created if missing), sorted.
pub fn ringtones() -> anyhow::Result<Vec<String>> {
    let dir = ringtones_dir();
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let mut files: Vec<String> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_audio(n))
        .collect();
    files.sort_by_key(|n| n.to_lowercase());
    Ok(files)
}

fn is_audio(name: &str) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    matches!(ext.as_str(), "wav" | "flac" | "ogg" | "oga" | "opus" | "mp3")
}

fn ringback_path(style: RingbackStyle, file: Option<&str>) -> anyhow::Result<PathBuf> {
    if style == RingbackStyle::Custom {
        let name = file.context("no ringing tone file chosen")?;
        // Only a plain name inside the ringtones folder.
        anyhow::ensure!(!name.contains('/') && !name.starts_with('.'), "invalid ringing tone name");
        let path = ringtones_dir().join(name);
        anyhow::ensure!(path.is_file(), "{} is missing", path.display());
        return Ok(path);
    }
    ensure(&format!("ringback-{style:?}").to_lowercase(), || ringback(style))
}

fn file_name(key: &str) -> anyhow::Result<String> {
    Ok(match key {
        "*" => "star".into(),
        "#" => "hash".into(),
        k if k.len() == 1 && k.chars().all(|c| c.is_ascii_digit()) => format!("dtmf-{k}"),
        k => anyhow::bail!("no tone for {k:?}"),
    })
}

fn ensure(name: &str, samples: impl FnOnce() -> Vec<f32>) -> anyhow::Result<PathBuf> {
    let dir = crate::config::xdg_dir("XDG_RUNTIME_DIR", ".cache").join("quattro-bt-phone");
    let path = dir.join(format!("{name}.wav"));
    if !path.exists() {
        std::fs::create_dir_all(&dir)?;
        write_wav(&path, &samples()).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(path)
}

/// The two DTMF frequencies of a key.
fn frequencies(key: &str) -> (f32, f32) {
    let (row, col) = match key {
        "1" => (0, 0),
        "2" => (0, 1),
        "3" => (0, 2),
        "4" => (1, 0),
        "5" => (1, 1),
        "6" => (1, 2),
        "7" => (2, 0),
        "8" => (2, 1),
        "9" => (2, 2),
        "*" => (3, 0),
        "0" => (3, 1),
        _ => (3, 2), // #
    };
    ([697.0, 770.0, 852.0, 941.0][row], [1209.0, 1336.0, 1477.0][col])
}

/// 120 ms of the key's DTMF pair with short fades, at a moderate level.
fn dtmf(key: &str) -> Vec<f32> {
    let (low, high) = frequencies(key);
    shaped(0.12, 0.005, |t| 0.18 * ((TAU * low * t).sin() + (TAU * high * t).sin()))
}

/// One cadence cycle of the ringing tone (on and off parts), looped by `Ringback`.
fn ringback(style: RingbackStyle) -> Vec<f32> {
    // (frequencies, [(seconds on, seconds off)]) from ITU-T E.180 and national practice.
    let (freqs, cadence): (&[f32], &[(f32, f32)]) = match style {
        RingbackStyle::Europe => (&[425.0], &[(1.0, 4.0)]),
        RingbackStyle::Uk => (&[400.0, 450.0], &[(0.4, 0.2), (0.4, 2.0)]),
        RingbackStyle::NorthAmerica => (&[440.0, 480.0], &[(2.0, 4.0)]),
        RingbackStyle::Chime => return chime(),
        RingbackStyle::Custom | RingbackStyle::Off => (&[], &[(0.0, 1.0)]),
    };
    let level = 0.25 / freqs.len().max(1) as f32;
    let mut out = Vec::new();
    for &(on, off) in cadence {
        out.extend(shaped(on, 0.01, |t| freqs.iter().map(|f| level * (TAU * f * t).sin()).sum()));
        out.extend(std::iter::repeat_n(0.0, (off * RATE as f32) as usize));
    }
    out
}

/// Two soft, decaying notes (E5, A5) and a pause: 3 s.
fn chime() -> Vec<f32> {
    let note = |f: f32| shaped(0.6, 0.004, move |t| 0.2 * (TAU * f * t).sin() * (-t * 6.0).exp());
    let mut out = note(659.3);
    out.truncate((0.3 * RATE as f32) as usize);
    out.extend(note(880.0));
    out.resize((3.0 * RATE as f32) as usize, 0.0);
    out
}

/// A 30 ms decaying tick: much quieter and shorter than a tone, for typing.
fn tick() -> Vec<f32> {
    shaped(0.03, 0.002, |t| 0.12 * (TAU * 1800.0 * t).sin() * (-t * 150.0).exp())
}

fn shaped(seconds: f32, fade: f32, wave: impl Fn(f32) -> f32) -> Vec<f32> {
    let n = (seconds * RATE as f32) as usize;
    (0..n)
        .map(|i| {
            let t = i as f32 / RATE as f32;
            let envelope = (t / fade).min(1.0).min((seconds - t) / fade).max(0.0);
            wave(t) * envelope
        })
        .collect()
}

/// 16-bit mono PCM WAV.
fn write_wav(path: &Path, samples: &[f32]) -> std::io::Result<()> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    let tmp = path.with_extension("wav.tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_map_to_files_and_frequencies() {
        assert_eq!(file_name("5").unwrap(), "dtmf-5");
        assert_eq!(file_name("#").unwrap(), "hash");
        assert!(file_name("x").is_err());
        assert!(file_name("12").is_err());
        assert_eq!(frequencies("0"), (941.0, 1336.0));
        assert_eq!(frequencies("#"), (941.0, 1477.0));
    }

    #[test]
    fn ringback_cycles_have_the_right_length() {
        let secs = |s| ringback(s).len() as f32 / RATE as f32;
        assert!((secs(RingbackStyle::Europe) - 5.0).abs() < 0.01);
        assert!((secs(RingbackStyle::Uk) - 3.0).abs() < 0.01);
        assert!((secs(RingbackStyle::NorthAmerica) - 6.0).abs() < 0.01);
        assert!((secs(RingbackStyle::Chime) - 3.0).abs() < 0.01);
        assert!(ringback(RingbackStyle::Uk).iter().all(|x| x.abs() <= 0.25));
    }

    #[test]
    fn only_audio_files_count_as_ringtones() {
        assert!(is_audio("Nokia tune.MP3"));
        assert!(is_audio("ring.ogg"));
        assert!(!is_audio("notes.txt"));
        assert!(!is_audio("noextension"));
        assert!(ringback_path(RingbackStyle::Custom, Some("../secret.wav")).is_err());
        assert!(ringback_path(RingbackStyle::Custom, None).is_err());
    }

    #[test]
    fn writes_a_pcm_wav() {
        let path = std::env::temp_dir().join("quattro-bt-phone-dtmf-test.wav");
        let samples = dtmf("7");
        write_wav(&path, &samples).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), RATE);
        assert_eq!(bytes.len(), 44 + samples.len() * 2);
    }

    #[test]
    fn sounds_stay_in_range_and_tick_is_quieter() {
        let peak = |s: &[f32]| s.iter().fold(0f32, |m, x| m.max(x.abs()));
        let tone = dtmf("1");
        let t = tick();
        assert!(peak(&tone) <= 0.37 && peak(&tone) > 0.2);
        assert!(peak(&t) < peak(&tone) / 2.0);
        assert!(t.len() < tone.len());
    }
}
