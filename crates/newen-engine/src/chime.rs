//! quill's own sounds, made here from two soft tones each: no recording and nobody else's
//! sound. A finished task falls a fourth to rest (G5 then D5); a session that needs the person
//! rises a fourth, as a question (D5 then G5). Each tone is a sine with a little of its
//! second and third harmonics, a 6 ms rise and a 90 ms decay; the second starts 120 ms after
//! the first, about 380 ms in all, at a third of full scale. They play through what the
//! system already has, fed the WAV on stdin or, where the player needs one, in a file.

use newen_model::Chime;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const RATE: u32 = 22_050;

/// A program to run, its arguments and its environment.
type Run = (&'static str, Vec<String>, Vec<(&'static str, String)>);

/// The chime as a 16-bit mono WAV.
pub(crate) fn wav(chime: Chime) -> Vec<u8> {
    const G5: f32 = 783.99;
    const D5: f32 = 587.33;
    let (first, second) = match chime {
        Chime::Done => (G5, D5),
        Chime::Ask => (D5, G5),
    };
    let rate = RATE as f32;
    let length = (0.38 * rate) as usize;
    let tone = |frequency: f32, t: f32| {
        if t < 0.0 {
            return 0.0;
        }
        let envelope = (t / 0.006).min(1.0) * (-t / 0.09).exp();
        let phase = std::f32::consts::TAU * frequency * t;
        envelope * (phase.sin() + 0.25 * (2.0 * phase).sin() + 0.08 * (3.0 * phase).sin())
    };
    let mut samples: Vec<f32> = (0..length)
        .map(|i| {
            let t = i as f32 / rate;
            tone(first, t) + 0.8 * tone(second, t - 0.12)
        })
        .collect();
    // The last 20 ms go to silence, so the end makes no click.
    let fade = (0.02 * rate) as usize;
    for (k, sample) in samples[length - fade..].iter_mut().enumerate() {
        *sample *= 1.0 - k as f32 / fade as f32;
    }
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let scale = 0.33 * f32::from(i16::MAX) / peak;
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|s| ((s * scale).round() as i16).to_le_bytes())
        .collect();
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); // PCM
    out.extend(1u16.to_le_bytes()); // mono
    out.extend(RATE.to_le_bytes());
    out.extend((RATE * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

/// The players to try in turn on this system: program, arguments and environment. On Linux
/// the WAV goes on stdin (`-` for `pw-play`); macOS's `afplay` takes the file at `path`;
/// Windows's `SoundPlayer` reads it from base64 in an environment variable.
pub(crate) fn players(wav: &[u8], path: &std::path::Path) -> Vec<Run> {
    if cfg!(target_os = "macos") {
        vec![("afplay", vec![path.display().to_string()], vec![])]
    } else if cfg!(windows) {
        use base64::Engine;
        let script = "$b=[Convert]::FromBase64String($env:NEWEN_SOUND);\
                      (New-Object Media.SoundPlayer([IO.MemoryStream]::new($b))).PlaySync()";
        vec![(
            "powershell",
            ["-NoProfile", "-NonInteractive", "-Command", script]
                .map(String::from)
                .to_vec(),
            vec![(
                "NEWEN_SOUND",
                base64::engine::general_purpose::STANDARD.encode(wav),
            )],
        )]
    } else {
        vec![
            ("pw-play", vec!["-".into()], vec![]),
            ("paplay", vec![], vec![]),
            ("aplay", vec!["-q".into()], vec![]),
        ]
    }
}

/// Play the chime with the first player that works; none, and nothing is heard.
pub(crate) async fn play(chime: Chime) {
    let wav = wav(chime);
    let path = std::env::temp_dir().join(match chime {
        Chime::Done => "quill-done.wav",
        Chime::Ask => "quill-ask.wav",
    });
    // A few kilobytes, on an engine thread, not the window's.
    if cfg!(target_os = "macos") && std::fs::write(&path, &wav).is_err() {
        return;
    }
    // Only Linux's players read the WAV from stdin.
    let fed = !cfg!(windows) && !cfg!(target_os = "macos");
    for (program, args, env) in players(&wav, &path) {
        let child = Command::from(newen_files::command(program))
            .args(args)
            .envs(env)
            .stdin(if fed { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn();
        let Ok(mut child) = child else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(&wav).await;
        }
        if child.wait().await.is_ok_and(|status| status.success()) {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples(wav: &[u8]) -> Vec<i16> {
        wav[44..]
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect()
    }

    #[test]
    fn each_chime_is_a_short_quiet_wav_of_its_own() {
        for chime in [Chime::Done, Chime::Ask] {
            let wav = wav(chime);
            assert_eq!(&wav[..4], b"RIFF");
            assert_eq!(&wav[8..16], b"WAVEfmt ");
            assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), RATE);
            let samples = samples(&wav);
            let ms = samples.len() as u32 * 1000 / RATE;
            assert!((250..=400).contains(&ms), "{ms} ms");
            let peak = samples.iter().map(|s| s.unsigned_abs()).max().unwrap();
            assert!(peak < i16::MAX as u16 / 2, "quiet: {peak}");
            // It starts and ends at rest, so there is no click.
            assert!(samples[0].abs() < 200 && samples.last().unwrap().abs() < 50);
        }
        assert_ne!(wav(Chime::Done), wav(Chime::Ask));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn on_linux_the_chime_goes_to_pipewire_then_pulse_then_alsa() {
        let programs: Vec<_> = players(&[], std::path::Path::new("x"))
            .into_iter()
            .map(|p| (p.0, p.1))
            .collect();
        assert_eq!(
            programs,
            [
                ("pw-play", vec!["-".to_owned()]),
                ("paplay", vec![]),
                ("aplay", vec!["-q".to_owned()])
            ]
        );
    }
}
