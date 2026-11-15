use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use serde_json::Value;

pub const SAMPLE_RATE: u32 = 48_000;
/// Frames per chunk: 20 ms of audio.
pub const CHUNK_FRAMES: usize = SAMPLE_RATE as usize / 50;
const CHUNK_BYTES: usize = CHUNK_FRAMES * 2 * 2;

/// The game's own sound, read from its PipeWire stream, so nothing else on the desktop is heard.
pub struct GameAudio {
    recorder: Child,
    chunks: Receiver<Result<Vec<i16>, String>>,
}

impl GameAudio {
    /// Starts recording the output stream of the process `pid`.
    pub fn open(pid: u32) -> Result<Self, String> {
        let dump = Command::new("pw-dump")
            .output()
            .map_err(|error| format!("Can't run pw-dump to find the game's sound: {error}"))?;
        let target = stream_serial(&String::from_utf8_lossy(&dump.stdout), pid)?
            .ok_or("The game isn't playing any sound. Turn its sound on in the game's settings.")?;
        let mut recorder = Command::new("pw-record")
            .args(["--target", &target, "--channels", "2", "--format", "s16"])
            .args(["--rate", &SAMPLE_RATE.to_string(), "--raw", "-"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("Can't run pw-record to hear the game: {error}"))?;
        let mut output = recorder
            .stdout
            .take()
            .ok_or("pw-record started without an output")?;
        let (sender, chunks) = mpsc::channel();
        // Ends when pw-record exits and its output closes, or when `chunks` is dropped.
        thread::spawn(move || {
            let mut bytes = vec![0; CHUNK_BYTES];
            loop {
                let chunk = match output.read_exact(&mut bytes) {
                    Ok(()) => Ok(bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|pair| i16::from_le_bytes(*pair))
                        .collect()),
                    Err(error) => Err(format!("Lost the game's sound: {error}")),
                };
                let failed = chunk.is_err();
                if sender.send(chunk).is_err() || failed {
                    return;
                }
            }
        });
        Ok(GameAudio { recorder, chunks })
    }

    /// The next 20 ms of interleaved stereo samples, or `None` if none arrive within `timeout`.
    pub fn next(&self, timeout: Duration) -> Result<Option<Vec<i16>>, String> {
        match self.chunks.recv_timeout(timeout) {
            Ok(chunk) => chunk.map(Some),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            Err(RecvTimeoutError::Disconnected) => Err("Lost the game's sound.".into()),
        }
    }
}

impl Drop for GameAudio {
    fn drop(&mut self) {
        if let Err(error) = self.recorder.kill() {
            eprintln!("pececito: stopping pw-record: {error}");
        }
        if let Err(error) = self.recorder.wait() {
            eprintln!("pececito: waiting for pw-record: {error}");
        }
    }
}

/// The `object.serial` of the first audio output stream that `pid` plays, from `pw-dump`'s JSON.
fn stream_serial(dump: &str, pid: u32) -> Result<Option<String>, String> {
    let objects: Vec<Value> = serde_json::from_str(dump)
        .map_err(|error| format!("Can't read pw-dump's output: {error}"))?;
    // PipeWire writes some properties as numbers and others as strings, depending on the client.
    let number = |value: &Value| match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    };
    Ok(objects.iter().find_map(|object| {
        let props = &object["info"]["props"];
        let is_output = props["media.class"] == "Stream/Output/Audio";
        let is_game = number(&props["application.process.id"]) == Some(pid.into());
        if object["type"] == "PipeWire:Interface:Node" && is_output && is_game {
            number(&props["object.serial"]).map(|serial| serial.to_string())
        } else {
            None
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"[
        {"id": 30, "type": "PipeWire:Interface:Node",
         "info": {"props": {"media.class": "Audio/Sink", "object.serial": 31}}},
        {"id": 94, "type": "PipeWire:Interface:Node",
         "info": {"props": {"media.class": "Stream/Output/Audio",
                            "application.process.id": "1234", "object.serial": 456}}},
        {"id": 95, "type": "PipeWire:Interface:Node",
         "info": {"props": {"media.class": "Stream/Output/Audio",
                            "application.process.id": 54820, "object.serial": 457}}},
        {"id": 96, "type": "PipeWire:Interface:Link", "info": null}
    ]"#;

    #[test]
    fn finds_the_stream_of_a_process() {
        assert_eq!(stream_serial(DUMP, 54820), Ok(Some("457".into())));
        assert_eq!(stream_serial(DUMP, 1234), Ok(Some("456".into())));
        assert_eq!(stream_serial(DUMP, 1), Ok(None));
        assert!(stream_serial("not json", 1).is_err());
    }
}
