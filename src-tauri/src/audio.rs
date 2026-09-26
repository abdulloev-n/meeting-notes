use std::{path::{Path, PathBuf}, sync::{atomic::{AtomicBool, Ordering}, mpsc, Arc}, thread::{self, JoinHandle}, time::{Duration, Instant}};
use chrono::{DateTime, Utc};
use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode};
use crate::models::Device;

#[cfg(test)]
const RATE: u32 = 16_000;

pub struct Session {
    pub started_at: DateTime<Utc>,
    pub directory: PathBuf,
    stop: Arc<AtomicBool>,
    mic: JoinHandle<Result<(), String>>,
    system: JoinHandle<Result<(), String>>,
}

fn enumerate(direction: Direction) -> Result<Vec<Device>, String> {
    wasapi::initialize_mta().ok().map_err(|e| e.to_string())?;
    let enumerator = DeviceEnumerator::new().map_err(|e| e.to_string())?;
    let collection = enumerator.get_device_collection(&direction).map_err(|e| e.to_string())?;
    let mut devices = Vec::new();
    for index in 0..collection.get_nbr_devices().map_err(|e| e.to_string())? {
        let device = collection.get_device_at_index(index).map_err(|e| e.to_string())?;
        devices.push(Device { id: device.get_id().map_err(|e| e.to_string())?, name: device.get_friendlyname().map_err(|e| e.to_string())? });
    }
    Ok(devices)
}

pub fn microphones() -> Vec<Device> { enumerate(Direction::Capture).unwrap_or_default() }
pub fn outputs() -> Vec<Device> { enumerate(Direction::Render).unwrap_or_default() }

fn capture(direction: Direction, device_id: String, path: PathBuf, stop: Arc<AtomicBool>, ready: mpsc::Sender<Result<(), String>>) -> Result<(), String> {
    let outcome = (|| -> Result<(), String> {
        wasapi::initialize_mta().ok().map_err(|e| e.to_string())?;
        let enumerator = DeviceEnumerator::new().map_err(|e| e.to_string())?;
        let device = if device_id.is_empty() { enumerator.get_default_device(&direction) } else { enumerator.get_device(&device_id) }.map_err(|e| e.to_string())?;
        let mut client = device.get_iaudioclient().map_err(|e| e.to_string())?;
        let format = client.get_mixformat().map_err(|e| format!("Read device audio format: {e}"))?;
        let sample_type = format.get_subformat().map_err(|e| e.to_string())?;
        let bits = format.get_bitspersample(); let channels = format.get_nchannels(); let rate = format.get_samplespersec();
        if !matches!((sample_type,bits),(SampleType::Float,32)|(SampleType::Int,16)|(SampleType::Int,24)|(SampleType::Int,32)) { return Err(format!("Unsupported device audio format: {sample_type:?} {bits}-bit")); }
        let mode = StreamMode::PollingShared { autoconvert: false, buffer_duration_hns: 2_000_000 };
        client.initialize_client(&format, &Direction::Capture, &mode).map_err(|e| format!("Start device audio stream: {e}"))?;
        let capture_client = client.get_audiocaptureclient().map_err(|e| e.to_string())?;
        let buffer_size = client.get_buffer_size().map_err(|e| e.to_string())? as usize;
        let spec = hound::WavSpec { channels, sample_rate: rate, bits_per_sample: bits, sample_format: if sample_type==SampleType::Float {hound::SampleFormat::Float}else{hound::SampleFormat::Int} };
        let mut writer = hound::WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
        let bytes_per_sample = (bits/8) as usize; let bytes_per_frame = format.get_blockalign() as usize;
        let mut buffer = vec![0u8; buffer_size * bytes_per_frame];
        client.start_stream().map_err(|e| e.to_string())?;
        ready.send(Ok(())).ok();
        let start = Instant::now(); let mut frames: usize = 0;
        while !stop.load(Ordering::SeqCst) {
            while capture_client.get_next_packet_size().map_err(|e| e.to_string())?.unwrap_or(0) > 0 {
                let (count, _) = capture_client.read_from_device(&mut buffer).map_err(|e| e.to_string())?;
                for bytes in buffer[..count as usize * bytes_per_frame].chunks_exact(bytes_per_sample) {
                    match (sample_type,bits) {
                        (SampleType::Float,32) => writer.write_sample(f32::from_le_bytes(bytes.try_into().unwrap())).map_err(|e|e.to_string())?,
                        (SampleType::Int,16) => writer.write_sample(i16::from_le_bytes(bytes.try_into().unwrap())).map_err(|e|e.to_string())?,
                        (SampleType::Int,24) => writer.write_sample(i32::from_le_bytes([bytes[0],bytes[1],bytes[2],if bytes[2]&0x80 != 0 {255}else{0}])).map_err(|e|e.to_string())?,
                        (SampleType::Int,32) => writer.write_sample(i32::from_le_bytes(bytes.try_into().unwrap())).map_err(|e|e.to_string())?,
                        _ => unreachable!(),
                    }
                }
                frames += count as usize;
            }
            let expected = (start.elapsed().as_secs_f64() * rate as f64) as usize;
            if expected > frames + (rate as usize / 10) {
                let pad = expected - frames - (rate as usize / 20);
                for _ in 0..pad * channels as usize { if sample_type==SampleType::Float {writer.write_sample(0f32).map_err(|e|e.to_string())?;}else if bits==16 {writer.write_sample(0i16).map_err(|e|e.to_string())?;}else{writer.write_sample(0i32).map_err(|e|e.to_string())?;} }
                frames += pad;
            }
            thread::sleep(Duration::from_millis(10));
        }
        client.stop_stream().map_err(|e| e.to_string())?;
        writer.finalize().map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(error) = &outcome { ready.send(Err(format!("{direction:?} capture: {error}"))).ok(); }
    outcome.map_err(|error|format!("{direction:?} capture: {error}"))
}

pub fn start(directory: PathBuf, microphone_id: String, system_audio_id: String) -> Result<Session, String> {
    std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::channel();
    let mic_stop = stop.clone(); let mic_tx = tx.clone(); let mic_dir = directory.clone();
    let mic = thread::Builder::new().name("microphone-capture".into()).spawn(move || capture(Direction::Capture, microphone_id, mic_dir.join("microphone.wav"), mic_stop, mic_tx)).map_err(|e| e.to_string())?;
    let sys_stop = stop.clone(); let sys_dir = directory.clone();
    let system = thread::Builder::new().name("system-capture".into()).spawn(move || capture(Direction::Render, system_audio_id, sys_dir.join("system.wav"), sys_stop, tx)).map_err(|e| e.to_string())?;
    for _ in 0..2 {
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(())) => {},
            Ok(Err(error)) => { stop.store(true, Ordering::SeqCst); let _ = mic.join(); let _ = system.join(); return Err(error); },
            Err(error) => { stop.store(true, Ordering::SeqCst); let _ = mic.join(); let _ = system.join(); return Err(format!("Audio device did not start: {error}")); }
        }
    }
    Ok(Session { started_at: Utc::now(), directory, stop, mic, system })
}

impl Session {
    pub fn finish(self, ffmpeg: &Path) -> Result<(DateTime<Utc>, PathBuf), String> {
        self.stop.store(true, Ordering::SeqCst);
        self.mic.join().map_err(|_| "Microphone capture thread stopped unexpectedly".to_string())??;
        self.system.join().map_err(|_| "System capture thread stopped unexpectedly".to_string())??;
        Ok((Utc::now(), mix_recording(&self.directory, ffmpeg)?))
    }
}

pub fn usable_raw_audio(directory: &Path) -> Option<PathBuf> {
    ["microphone.wav", "system.wav"].into_iter().map(|name| directory.join(name))
        .find(|path| path.metadata().map(|metadata| metadata.len() > 44).unwrap_or(false))
}

pub fn mix_recording(directory: &Path, ffmpeg: &Path) -> Result<PathBuf, String> {
    let output = directory.join("recording.m4a");
    if output.metadata().map(|metadata| metadata.len() > 0).unwrap_or(false) { return Ok(output); }
    let mic = directory.join("microphone.wav"); let system = directory.join("system.wav");
    let available: Vec<_> = [&mic, &system].into_iter().filter(|path| path.metadata().map(|metadata| metadata.len() > 44).unwrap_or(false)).collect();
    if available.is_empty() { return Err("Audio capture produced no data".into()); }
    let temporary = directory.join("recording.tmp.m4a");
    let mut command = std::process::Command::new(ffmpeg);
    command.args(["-nostdin", "-y", "-loglevel", "error"]);
    for path in &available { command.arg("-i").arg(path); }
    if available.len() == 2 { command.args(["-filter_complex", "[0:a][1:a]amix=inputs=2:duration=longest:dropout_transition=0"]); }
    let result = command.args(["-ac", "1", "-ar", "16000", "-c:a", "aac", "-b:a", "48k"]).arg(&temporary).output().map_err(|e| e.to_string())?;
    if !result.status.success() { return Err(format!("Audio conversion failed: {}", String::from_utf8_lossy(&result.stderr))); }
    if temporary.metadata().map_err(|e| e.to_string())?.len() == 0 { return Err("Audio conversion produced an empty file".into()); }
    std::fs::rename(&temporary, &output).map_err(|e| e.to_string())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn converts_and_recovers_single_source_audio() {
        let directory = std::env::temp_dir().join(format!("meeting-notes-audio-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let spec = hound::WavSpec { channels: 1, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut writer = hound::WavWriter::create(directory.join("microphone.wav"), spec).unwrap();
        for index in 0..RATE { writer.write_sample(((index as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin() * 3000.0) as i16).unwrap(); }
        writer.finalize().unwrap();
        let ffmpeg = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bin").join("ffmpeg.exe");
        let output = mix_recording(&directory, &ffmpeg).unwrap();
        assert!(output.metadata().unwrap().len() > 1000);
        assert_eq!(mix_recording(&directory, &ffmpeg).unwrap(), output);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    #[ignore = "Requires microphone and output stream access on the test machine"]
    fn captures_available_windows_audio_devices() {
        if microphones().is_empty() || outputs().is_empty() { return; }
        let directory = std::env::temp_dir().join(format!("meeting-notes-capture-{}", uuid::Uuid::new_v4()));
        let session = start(directory.clone(), String::new(), String::new()).unwrap();
        std::thread::sleep(Duration::from_millis(750));
        let ffmpeg = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bin").join("ffmpeg.exe");
        let (_, output) = session.finish(&ffmpeg).unwrap();
        assert!(output.metadata().unwrap().len() > 1000);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
