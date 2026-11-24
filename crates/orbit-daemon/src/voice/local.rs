//! Optional installed inference tools and OS audio writers. Keep text and audio off argv.
use super::{audio, endpoint::Transcript};
use anyhow::{Context, Result, ensure};
use orbit_protocol::{VoiceHostCapabilities, VoiceSttBackend};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

fn binary(names: &[&str]) -> Option<PathBuf> {
    names
        .iter()
        .find_map(|name| orbit_providers::find_executable(name))
}
const WHISPER: &[&str] = &["whisper-cli", "whisper.cpp"];
const SENSE: &[&str] = &["sherpa-onnx-offline"];
const KOKORO: &[&str] = &["kokoro-tts"];

fn native_binary() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Some(PathBuf::from("/usr/bin/say")).filter(|p| p.is_file())
    }
    #[cfg(windows)]
    {
        binary(&["powershell.exe"])
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        binary(&["espeak-ng", "espeak"])
    }
}

pub(super) fn capabilities() -> VoiceHostCapabilities {
    VoiceHostCapabilities {
        local_whisper: binary(WHISPER).is_some(),
        local_sense_voice: binary(SENSE).is_some(),
        local_kokoro: binary(KOKORO).is_some(),
        system_native_tts: native_binary().is_some(),
        system_client_fallback: true,
    }
}

fn tempdir() -> Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("orbit-voice-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o700));
    }
    Ok(builder.tempdir()?)
}

async fn run(mut cmd: Command, input: &[u8], capture_stderr: bool) -> Result<(Vec<u8>, Vec<u8>)> {
    cmd.kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(if capture_stderr {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().context("missing voice tool stdin")?;
    let stdout = child.stdout.take().context("missing voice tool stdout")?;
    let stderr = child.stderr.take();
    let outcome = tokio::time::timeout(Duration::from_secs(90), async {
        let write_fut = async {
            stdin.write_all(input).await?;
            drop(stdin);
            anyhow::Ok(())
        };
        let stdout_fut = async {
            let mut out = Vec::new();
            stdout.take(65537).read_to_end(&mut out).await?;
            ensure!(out.len() <= 65536, "oversized voice tool output");
            anyhow::Ok(out)
        };
        let stderr_fut = async {
            let mut err = Vec::new();
            if let Some(s) = stderr {
                s.take(65537).read_to_end(&mut err).await?;
                if err.len() > 65536 {
                    err.truncate(65536);
                }
            }
            anyhow::Ok(err)
        };
        let ((), out, err) = tokio::try_join!(write_fut, stdout_fut, stderr_fut)?;
        ensure!(child.wait().await?.success(), "voice tool failed");
        anyhow::Ok((out, err))
    })
    .await;
    if !matches!(outcome, Ok(Ok(_))) {
        let _ = child.kill().await;
    }
    outcome?
}

fn parse_sensevoice_transcript(stdout: &[u8], stderr: &[u8]) -> Result<Transcript> {
    if let Ok(t) = serde_json::from_slice::<Transcript>(stdout) {
        return Ok(t);
    }
    for bytes in [stderr, stdout] {
        let text = String::from_utf8_lossy(bytes);
        for line in text.lines().rev() {
            let trimmed = line.trim();
            if trimmed.starts_with('{')
                && trimmed.ends_with('}')
                && let Ok(t) = serde_json::from_str::<Transcript>(trimmed)
            {
                return Ok(t);
            }
        }
    }
    anyhow::bail!("could not parse SenseVoice JSON transcript from output")
}

pub(super) async fn transcribe(
    backend: VoiceSttBackend,
    model: &Path,
    wav: &[u8],
    lang: Option<&str>,
) -> Result<Transcript> {
    let dir = tokio::task::spawn_blocking(tempdir).await??;
    let input = dir.path().join("capture.wav");
    tokio::fs::write(&input, wav).await?;
    let mut cmd = match backend {
        VoiceSttBackend::LocalWhisper => {
            let mut cmd = Command::new(binary(WHISPER).context("whisper-cli unavailable")?);
            cmd.arg("-m")
                .arg(model)
                .arg("-l")
                .arg(lang.unwrap_or("auto"))
                .args(["-nt", "-np", "-f"]);
            cmd
        }
        VoiceSttBackend::LocalSenseVoice => {
            let mut cmd = Command::new(binary(SENSE).context("sherpa-onnx-offline unavailable")?);
            let mut model_arg = std::ffi::OsString::from("--sense-voice-model=");
            model_arg.push(model.as_os_str());
            let mut tokens_arg = std::ffi::OsString::from("--tokens=");
            tokens_arg.push(model.with_file_name("tokens.txt").as_os_str());
            cmd.arg(model_arg)
                .arg(tokens_arg)
                .arg(format!("--sense-voice-language={}", lang.unwrap_or("auto")));
            cmd
        }
        _ => anyhow::bail!("unsupported local STT"),
    };
    cmd.arg(input).current_dir(dir.path());
    let (output, err_output) = run(cmd, &[], backend == VoiceSttBackend::LocalSenseVoice).await?;
    if backend == VoiceSttBackend::LocalSenseVoice {
        let mut transcript = parse_sensevoice_transcript(&output, &err_output)?;
        transcript.language = transcript
            .language
            .map(|lang| {
                lang.trim_start_matches("<|")
                    .trim_end_matches("|>")
                    .to_owned()
            })
            .filter(|lang| !lang.is_empty());
        return Ok(transcript);
    }
    Ok(Transcript {
        text: String::from_utf8(output)?.trim().into(),
        language: lang.map(str::to_owned),
    })
}

pub(super) async fn kokoro(
    model: &Path,
    text: &str,
    language: Option<&str>,
    voice: Option<&str>,
) -> Result<Vec<u8>> {
    let dir = tokio::task::spawn_blocking(tempdir).await??;
    let output = dir.path().join("speech.wav");
    let (lang, default_voice) = match language {
        Some("zh") => ("cmn", "zf_xiaobei"),
        Some("ja") => ("ja", "jf_alpha"),
        _ => ("en-us", "af_heart"),
    };
    let mut cmd = Command::new(binary(KOKORO).context("kokoro-tts unavailable")?);
    cmd.current_dir(dir.path())
        .arg("-")
        .arg(&output)
        .arg("--model")
        .arg(model)
        .arg("--voices")
        .arg(model.with_file_name("voices-v1.0.bin"))
        .args([
            "--format",
            "wav",
            "--lang",
            lang,
            "--voice",
            voice.unwrap_or(default_voice),
        ]);
    run(cmd, text.as_bytes(), false).await?;
    let out_path = output.clone();
    let wav = tokio::task::spawn_blocking(move || audio::read_audio_file(&out_path)).await??;
    drop(dir);
    Ok(wav)
}

pub(super) async fn native(
    text: &str,
    language: Option<&str>,
    voice: Option<&str>,
) -> Result<Vec<u8>> {
    let dir = tokio::task::spawn_blocking(tempdir).await??;
    let output = dir.path().join("speech.wav");
    let bin = native_binary().context("native speech unavailable")?;
    let build_cmd = |selected_voice: Option<&str>| {
        let mut cmd = Command::new(&bin);
        #[cfg(target_os = "macos")]
        {
            cmd.arg("-o").arg(&output).args([
                "--file-format=WAVE",
                "--data-format=LEI16@16000",
                "-f",
                "-",
            ]);
            if let Some(v) = selected_voice {
                cmd.arg("-v").arg(v);
            }
        }
        #[cfg(windows)]
        {
            cmd.env("ORBIT_SPEECH_FILE",&output).env("ORBIT_SPEECH_VOICE",selected_voice.unwrap_or("")).env("ORBIT_SPEECH_LANG",language.unwrap_or(""))
                .args(["-NoLogo","-NoProfile","-NonInteractive","-Command", r#"$ErrorActionPreference='Stop'; [Console]::InputEncoding=[Text.UTF8Encoding]::new($false); Add-Type -AssemblyName System.Speech; $s=New-Object System.Speech.Synthesis.SpeechSynthesizer; try { if ($env:ORBIT_SPEECH_VOICE) { $s.SelectVoice($env:ORBIT_SPEECH_VOICE) } elseif ($env:ORBIT_SPEECH_LANG) { $v=$s.GetInstalledVoices() | Where-Object { $_.Enabled -and $_.VoiceInfo.Culture.TwoLetterISOLanguageName -eq $env:ORBIT_SPEECH_LANG } | Select-Object -First 1; if ($v) { $s.SelectVoice($v.VoiceInfo.Name) } }; $s.SetOutputToWaveFile($env:ORBIT_SPEECH_FILE); $s.Speak([Console]::In.ReadToEnd()) } finally { $s.Dispose() }"#]);
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            cmd.arg("--stdin").arg("-w").arg(&output);
            if let Some(v) = selected_voice.or(language) {
                cmd.arg("-v").arg(v);
            }
        }
        cmd
    };
    let preferred_voice = voice.or(if cfg!(target_os = "macos") {
        match language {
            Some("zh") => Some("Tingting"),
            Some("ja") => Some("Kyoko"),
            _ => None,
        }
    } else {
        None
    });
    if let Err(err) = run(build_cmd(preferred_voice), text.as_bytes(), false).await {
        // Retry with system default voice on macOS if optional localized voice is absent.
        if cfg!(target_os = "macos") && voice.is_none() && preferred_voice.is_some() {
            run(build_cmd(None), text.as_bytes(), false).await?;
        } else {
            return Err(err);
        }
    }
    let out_path = output.clone();
    let wav = tokio::task::spawn_blocking(move || audio::read_audio_file(&out_path)).await??;
    drop(dir);
    Ok(wav)
}
