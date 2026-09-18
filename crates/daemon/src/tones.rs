//! Keypad feedback: the DTMF tone of a clicked key, or a soft tick for a typed one.
//!
//! The shell can't play sound, so the daemon does, with `pw-play` on the call speakers. The
//! sounds are synthesised once into `$XDG_RUNTIME_DIR/quattro-bt-phone/`.

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::Context;
use tokio::process::Command;

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
