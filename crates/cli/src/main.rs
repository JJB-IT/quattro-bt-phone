//! `quattro-bt-phone`: command-line client for quattro-bt-phoned (scripts, keybinds, debugging).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand, ValueEnum};
use qbp_proto::{AudioDirection, AudioRoute, Command, Message, Request, RingbackStyle, SimEvent, State};

/// Control phone calls through quattro-bt-phoned.
#[derive(Parser, Debug)]
#[command(version, about)]
struct Args {
    /// Print raw JSON instead of text.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Show the phone and any calls.
    Status,
    /// Print every state change as JSON lines until interrupted.
    Watch,
    /// Call a number. Asks for confirmation unless --yes is given.
    Dial {
        number: String,
        /// Don't ask; required when not run from a terminal (scripts, keybinds).
        #[arg(long, short)]
        yes: bool,
    },
    /// Answer the ringing call.
    Answer,
    /// Reject the ringing call.
    Decline,
    /// Hang up the active call.
    Hangup,
    /// Hang up every call.
    HangupAll,
    /// Send DTMF tones on the active call.
    Tones { digits: String },
    /// Hold or resume the active call.
    Hold,
    /// Swap between the active and the held call.
    Swap,
    /// Move call audio to the laptop or the phone.
    Route { to: Route },
    /// Mute or unmute your microphone.
    Mute {
        #[arg(action = clap::ArgAction::Set, value_parser = clap::builder::BoolishValueParser::new())]
        on: bool,
    },
    /// List the speakers and microphones calls can use.
    AudioDevices,
    /// Use a speaker or microphone for calls (a name from `audio-devices`); no name = the
    /// system default.
    AudioDevice { direction: Direction, name: Option<String> },
    /// Refresh contacts and call history from the phone.
    Sync,
    /// Search contacts.
    Contacts { query: Option<String> },
    /// Recent calls.
    Recents {
        #[arg(long)]
        missed: bool,
    },
    /// Use this Bluetooth device as the phone.
    Select { address: String },
    /// Connect the phone.
    Connect,
    /// Ask the phone to allow calls (hands-free).
    AllowCalls,
    /// Ask the phone to allow contacts and call history.
    AllowContacts,
    /// Turn automatic call recording on or off.
    AutoRecord {
        #[arg(action = clap::ArgAction::Set, value_parser = clap::builder::BoolishValueParser::new())]
        on: bool,
    },
    /// Turn the dialling tones on or off.
    KeypadSounds {
        #[arg(action = clap::ArgAction::Set, value_parser = clap::builder::BoolishValueParser::new())]
        on: bool,
    },
    /// The ringing tone you hear while an outgoing call rings. `custom` takes a file name
    /// from `ringtones`.
    Ringback { style: Ringback, file: Option<String> },
    /// List your own ringing tones and the folder they go in.
    Ringtones,
    /// Pick an audio file as the ringing tone, in the desktop's file chooser.
    ChooseRingtone,
    /// Simulate phone events (daemon must run with --mock).
    Simulate { event: Sim, number: Option<String> },
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum Route {
    Laptop,
    Phone,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum Ringback {
    /// 425 Hz, long single ring (most of Europe).
    Europe,
    /// 400+450 Hz double ring (UK, Ireland, South Africa, Australia).
    Uk,
    /// 440+480 Hz.
    NorthAmerica,
    /// Two soft rising notes.
    Chime,
    /// Your own file (see `ringtones`).
    Custom,
    Off,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum Direction {
    Output,
    Input,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
enum Sim {
    Ring,
    RemoteAnswer,
    RemoteHangup,
    Disconnect,
    ResetSetup,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let command = match args.cmd {
        Cmd::Status => Command::GetState,
        Cmd::Watch => Command::Subscribe,
        Cmd::Dial { number, yes } => {
            confirm_dial(&number, yes)?;
            Command::Dial { number }
        }
        Cmd::Answer => Command::Answer { call: None },
        Cmd::Decline => Command::Decline { call: None },
        Cmd::Hangup => Command::Hangup { call: None },
        Cmd::HangupAll => Command::HangupAll,
        Cmd::Tones { digits } => Command::Tones { digits },
        Cmd::Hold => Command::Hold,
        Cmd::Swap => Command::Swap,
        Cmd::Route { to } => Command::SetRoute {
            route: match to {
                Route::Laptop => AudioRoute::Laptop,
                Route::Phone => AudioRoute::Phone,
            },
        },
        Cmd::Mute { on } => Command::SetMuted { muted: on },
        Cmd::AudioDevices => Command::GetAudioDevices,
        Cmd::AudioDevice { direction, name } => Command::SetAudioDevice {
            direction: match direction {
                Direction::Output => AudioDirection::Output,
                Direction::Input => AudioDirection::Input,
            },
            name,
        },
        Cmd::Sync => Command::Sync,
        Cmd::Contacts { query } => Command::GetContacts { query },
        Cmd::Recents { missed } => Command::GetRecents { missed_only: missed },
        Cmd::Select { address } => Command::SelectPhone { address },
        Cmd::Connect => Command::Connect,
        Cmd::AllowCalls => Command::RequestCalls,
        Cmd::AllowContacts => Command::RequestContacts,
        Cmd::AutoRecord { on } => Command::SetAutoRecord { enabled: on },
        Cmd::KeypadSounds { on } => Command::SetKeypadSounds { enabled: on },
        Cmd::Ringtones => Command::GetRingtones,
        Cmd::ChooseRingtone => Command::ChooseRingtone,
        Cmd::Ringback { style, file } => Command::SetRingback {
            file,
            style: match style {
                Ringback::Europe => RingbackStyle::Europe,
                Ringback::Uk => RingbackStyle::Uk,
                Ringback::NorthAmerica => RingbackStyle::NorthAmerica,
                Ringback::Chime => RingbackStyle::Chime,
                Ringback::Custom => RingbackStyle::Custom,
                Ringback::Off => RingbackStyle::Off,
            },
        },
        Cmd::Simulate { event, number } => Command::Simulate {
            event: match event {
                Sim::Ring => SimEvent::Ring,
                Sim::RemoteAnswer => SimEvent::RemoteAnswer,
                Sim::RemoteHangup => SimEvent::RemoteHangup,
                Sim::Disconnect => SimEvent::Disconnect,
                Sim::ResetSetup => SimEvent::ResetSetup,
            },
            number,
        },
    };
    let watch = matches!(command, Command::Subscribe);

    let path = qbp_proto::socket_path();
    let mut stream = UnixStream::connect(&path)
        .with_context(|| format!("can't reach quattro-bt-phoned at {} — is it running?", path.display()))?;
    let mut line = serde_json::to_vec(&Request { id: Some(1), command })?;
    line.push(b'\n');
    stream.write_all(&line)?;

    for line in BufReader::new(stream).lines() {
        let line = line?;
        if args.json {
            println!("{line}");
        }
        match serde_json::from_str::<Message>(&line)? {
            Message::Reply { ok: false, error, .. } => bail!(error.unwrap_or_else(|| "failed".into())),
            Message::Reply { .. } if !watch => return Ok(()),
            Message::Reply { .. } => {}
            _ if args.json => {}
            Message::State(s) => print_state(&s),
            Message::Contacts { contacts } => {
                for c in contacts {
                    let numbers: Vec<_> =
                        c.numbers.iter().map(|n| format!("{} {}", n.label, n.number)).collect();
                    println!("{:<28} {}", c.name, numbers.join(", "));
                }
            }
            Message::Recents { entries } => {
                for r in entries {
                    let who = r.name.unwrap_or(r.number);
                    let dur = r.duration.map(|d| format!("{}:{:02}", d / 60, d % 60)).unwrap_or_default();
                    println!("{:<9} {:<28} {dur}", format!("{:?}", r.kind).to_lowercase(), who);
                }
            }
            Message::AudioDevices { outputs, inputs } => {
                for (title, list) in [("Outputs", outputs), ("Inputs", inputs)] {
                    println!("{title}:");
                    for d in list {
                        println!("  {:<40} {}", d.description, d.name);
                    }
                }
            }
            Message::Ringtones { dir, files } => {
                println!("{dir}");
                for f in files {
                    println!("  {f}");
                }
            }
            Message::Recordings { recordings } => {
                for r in recordings {
                    println!("{}  {}", r.path, r.name.unwrap_or(r.number));
                }
            }
        }
    }
    Ok(())
}

/// Placing a call reaches a real person, so never dial by accident from a script or a test.
fn confirm_dial(number: &str, yes: bool) -> anyhow::Result<()> {
    use std::io::IsTerminal;
    if yes {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        bail!("refusing to dial {number} without --yes when not run from a terminal");
    }
    eprint!("Call {number}? [y/N] ");
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        bail!("not dialling");
    }
    Ok(())
}

fn print_state(s: &State) {
    let p = &s.phone;
    if p.address.is_empty() {
        println!("phone:    none selected");
    } else {
        println!("phone:    {} ({})", p.name, p.address);
        println!(
            "          paired={} connected={} calls={:?} contacts={:?}",
            p.paired, p.connected, p.calls, p.contacts
        );
    }
    println!("setup:    {:?}", s.setup_stage());
    for c in &s.calls {
        let who = c.name.clone().unwrap_or_else(|| c.number.clone());
        println!("call:     {:?} {:?} {who}", c.state, c.direction);
    }
    if !s.calls.is_empty() {
        println!("audio:    {:?}{}", s.audio.route, if s.audio.muted { ", muted" } else { "" });
    }
    let default = || "system default".to_string();
    println!(
        "devices:  output {}, input {}",
        s.settings.audio_output.clone().unwrap_or_else(default),
        s.settings.audio_input.clone().unwrap_or_else(default)
    );
    if let Some(r) = &s.recording {
        println!("recording {}", r.path);
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn arguments_are_well_formed() {
        Args::command().debug_assert();
        let args = Args::try_parse_from(["quattro-bt-phone", "mute", "on"]).unwrap();
        assert!(matches!(args.cmd, Cmd::Mute { on: true }));
        let args = Args::try_parse_from(["quattro-bt-phone", "auto-record", "off"]).unwrap();
        assert!(matches!(args.cmd, Cmd::AutoRecord { on: false }));
    }
}
