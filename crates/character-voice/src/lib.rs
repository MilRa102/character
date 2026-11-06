//! Local push-to-talk voice input and speech output.
//!
//! Audio is captured only while a [`Recording`] is alive. Temporary audio and
//! transcription files are removed when recording processing completes or fails.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use serde::Deserialize;

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, thiserror::Error)]
pub enum VoiceError {
    #[error("could not access voice model configuration: {0}")]
    Configuration(String),
    #[error("could not start {program}: {source}")]
    Start {
        program: &'static str,
        #[source]
        source: io::Error,
    },
    #[error("{program} failed: {details}")]
    Command {
        program: &'static str,
        details: String,
    },
    #[error("Ollama audio request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Local Ollama and optional Piper configuration from environment variables.
#[derive(Debug, Clone)]
pub struct VoiceConfig {
    pub ollama_url: String,
    pub ollama_model: String,
    pub piper_model: Option<PathBuf>,
}

impl VoiceConfig {
    pub fn from_env() -> Self {
        let ollama_url = std::env::var("CHARACTER_OLLAMA_URL")
            .unwrap_or_else(|_| "http://localhost:11434".into());
        let ollama_model =
            std::env::var("CHARACTER_OLLAMA_MODEL").unwrap_or_else(|_| "gemma4:e4b".into());
        let piper_model = std::env::var_os("CHARACTER_PIPER_MODEL").map(PathBuf::from);

        Self {
            ollama_url,
            ollama_model,
            piper_model,
        }
    }
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

/// An active microphone recording. Dropping it stops capture and removes its file.
#[derive(Debug)]
pub struct Recording {
    child: Option<Child>,
    audio: TempFile,
}

impl Recording {
    /// Starts a mono 16 kHz WAV recording through the system's default ALSA input.
    pub fn start() -> Result<Self, VoiceError> {
        let audio = TempFile::new("wav")?;
        let child = Command::new("arecord")
            .args(["-q", "-f", "S16_LE", "-r", "16000", "-c", "1", "-t", "wav"])
            .arg(&audio.path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|source| VoiceError::Start {
                program: "arecord",
                source,
            })?;

        Ok(Self {
            child: Some(child),
            audio,
        })
    }

    /// Stops capture and transcribes the recording through Ollama's audio API.
    pub fn transcribe(mut self, config: &VoiceConfig) -> Result<String, VoiceError> {
        self.stop_capture()?;

        let audio = fs::read(&self.audio.path)?;
        let endpoint = format!(
            "{}/v1/audio/transcriptions",
            config.ollama_url.trim_end_matches('/')
        );
        let response = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(180))
            .build()?
            .post(endpoint)
            .multipart(
                reqwest::blocking::multipart::Form::new()
                    .text("model", config.ollama_model.clone())
                    .part(
                        "file",
                        reqwest::blocking::multipart::Part::bytes(audio)
                            .file_name("recording.wav")
                            .mime_str("audio/wav")?,
                    ),
            )
            .send()?;

        if !response.status().is_success() {
            return Err(VoiceError::Command {
                program: "Ollama audio transcription",
                details: format!("{}: {}", response.status(), response.text()?),
            });
        }

        let transcript = response.json::<TranscriptionResponse>()?.text;
        let transcript = transcript.trim().to_owned();
        if transcript.is_empty() {
            return Err(VoiceError::Command {
                program: "Ollama audio transcription",
                details: "no speech was recognized".into(),
            });
        }

        Ok(transcript)
    }

    fn stop_capture(&mut self) -> Result<(), VoiceError> {
        let Some(mut child) = self.child.take() else {
            return Ok(());
        };

        let running = match child.try_wait() {
            Ok(status) => status.is_none(),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.into());
            }
        };
        if running {
            let pid = child.id();
            let interrupted = Command::new("kill")
                .args(["-INT", &pid.to_string()])
                .status()
                .is_ok_and(|status| status.success());
            if !interrupted
                && let Err(error) = child.kill()
                && error.kind() != io::ErrorKind::InvalidInput
            {
                let _ = child.wait();
                return Err(error.into());
            }
        }
        let status = child.wait()?;
        if !status.success() {
            return Err(VoiceError::Command {
                program: "arecord",
                details: format!("exited with {status}"),
            });
        }
        Ok(())
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            if let Err(error) = child.kill()
                && error.kind() != io::ErrorKind::InvalidInput
            {
                tracing::warn!(%error, "could not stop microphone capture during cleanup");
            }
            if let Err(error) = child.wait() {
                tracing::warn!(%error, "could not reap microphone capture process");
            }
        }
    }
}

/// Synthesizes and plays a reply with Piper and PipeWire.
pub fn speak(model: &Path, text: &str) -> Result<(), VoiceError> {
    if text.trim().is_empty() {
        return Ok(());
    }
    let audio = TempFile::new("wav")?;
    let mut child = Command::new("piper")
        .args(["--model"])
        .arg(model)
        .args(["--output_file"])
        .arg(&audio.path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| VoiceError::Start {
            program: "piper",
            source,
        })?;

    let Some(mut stdin) = child.stdin.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(VoiceError::Command {
            program: "piper",
            details: "standard input was not available".into(),
        });
    };
    let write_result = stdin.write_all(text.as_bytes());
    drop(stdin);
    if let Err(error) = write_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.into());
    }

    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(VoiceError::Command {
            program: "piper",
            details: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }

    let status = Command::new("pw-play")
        .arg(&audio.path)
        .status()
        .map_err(|source| VoiceError::Start {
            program: "pw-play",
            source,
        })?;
    if !status.success() {
        return Err(VoiceError::Command {
            program: "pw-play",
            details: format!("exited with {status}"),
        });
    }
    Ok(())
}

#[derive(Debug)]
struct TempFile {
    path: PathBuf,
}

impl TempFile {
    fn new(extension: &str) -> Result<Self, VoiceError> {
        let directory = std::env::temp_dir();
        for _ in 0..10 {
            let sequence = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
            let path = directory.join(format!(
                "character-{}-{sequence}.{extension}",
                std::process::id()
            ));
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            match options.open(&path) {
                Ok(_) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Err(VoiceError::Configuration(
            "could not allocate a unique temporary audio file".into(),
        ))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        remove_temp_file(&self.path);
        if self
            .path
            .extension()
            .is_some_and(|extension| extension == "wav")
        {
            remove_temp_file(&self.path.with_extension("txt"));
        }
    }
}

fn remove_temp_file(path: &Path) {
    if let Err(error) = fs::remove_file(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!(path = %path.display(), %error, "could not remove temporary voice file");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Read},
        net::TcpListener,
        thread,
    };

    #[test]
    fn temporary_audio_file_is_private_and_removed_on_drop() {
        for extension in ["wav", "txt"] {
            let temp = TempFile::new(extension).unwrap();
            let path = temp.path.clone();
            assert!(path.is_file());

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }

            drop(temp);
            assert!(!path.exists());
        }
    }

    #[test]
    fn transcribes_recording_through_ollama_audio_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut request_line = String::new();
            reader.read_line(&mut request_line).unwrap();
            assert!(request_line.starts_with("POST /v1/audio/transcriptions "));

            let mut content_length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" {
                    break;
                }
                if let Some((name, value)) = header.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    content_length = value.trim().parse::<usize>().unwrap();
                }
            }
            let mut body = vec![0; content_length];
            reader.read_exact(&mut body).unwrap();
            assert!(
                body.windows(b"gemma4:e4b".len())
                    .any(|window| window == b"gemma4:e4b")
            );
            assert!(
                body.windows(b"test-audio".len())
                    .any(|window| window == b"test-audio")
            );

            let response = r#"{"text":"hello there"}"#;
            write!(
                reader.get_mut(),
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.len(),
                response
            )
            .unwrap();
        });

        let audio = TempFile::new("wav").unwrap();
        fs::write(&audio.path, b"test-audio").unwrap();
        let path = audio.path.clone();
        let recording = Recording { child: None, audio };
        let config = VoiceConfig {
            ollama_url: format!("http://{address}"),
            ollama_model: "gemma4:e4b".into(),
            piper_model: None,
        };

        assert_eq!(recording.transcribe(&config).unwrap(), "hello there");
        assert!(!path.exists());
        server.join().unwrap();
    }
}
