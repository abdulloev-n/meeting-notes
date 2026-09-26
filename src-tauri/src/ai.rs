use std::{future::Future, path::{Path, PathBuf}, process::Command};
use reqwest::{multipart, Client};
use serde_json::{json, Value};
use crate::models::SummaryOutput;

const BASE: &str = "https://api.openai.com/v1";

pub trait AIProvider {
    fn transcribe(&self, audio: &Path, ffmpeg: &Path, model: &str) -> impl Future<Output=Result<String, String>> + Send;
    fn summarize(&self, transcript: &str, model: &str, language: &str) -> impl Future<Output=Result<SummaryOutput, String>> + Send;
}

pub struct OpenAIProvider { client: Client, key: String }

impl OpenAIProvider {
    pub fn new(key: String) -> Self { Self { client: Client::new(), key } }
    pub async fn test_key(key: &str) -> Result<(), String> {
        let response = Client::new().get(format!("{BASE}/models")).bearer_auth(key).send().await.map_err(|e| e.to_string())?;
        if response.status().is_success() { Ok(()) } else { Err(format!("OpenAI rejected the key (HTTP {}). The current key was kept.", response.status().as_u16())) }
    }
    async fn transcribe_chunk(&self, path: PathBuf, model: &str) -> Result<String, String> {
        let data = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("audio.m4a").to_string();
        let part = multipart::Part::bytes(data).file_name(name).mime_str("audio/mp4").map_err(|e| e.to_string())?;
        let form = multipart::Form::new().text("model", model.to_string()).text("response_format", "json").part("file", part);
        let response = self.client.post(format!("{BASE}/audio/transcriptions")).bearer_auth(&self.key).multipart(form).send().await.map_err(|e| e.to_string())?;
        let status = response.status(); let body: Value = response.json().await.map_err(|e| e.to_string())?;
        if !status.is_success() { return Err(format!("Transcription request failed (HTTP {}): {}", status.as_u16(), api_error(&body))); }
        body.get("text").and_then(Value::as_str).map(str::to_string).ok_or_else(|| "Transcription response contained no text".into())
    }
}

impl AIProvider for OpenAIProvider {
    async fn transcribe(&self, audio: &Path, ffmpeg: &Path, model: &str) -> Result<String, String> {
        let chunks_dir = audio.parent().ok_or("Recording folder missing")?.join("transcription_chunks");
        std::fs::create_dir_all(&chunks_dir).map_err(|e| e.to_string())?;
        let pattern = chunks_dir.join("part_%03d.m4a");
        let output = Command::new(ffmpeg).args(["-nostdin","-y","-loglevel","error","-i"]).arg(audio)
            .args(["-map","0:a:0","-c","copy","-f","segment","-segment_time","900","-reset_timestamps","1"]).arg(&pattern)
            .output().map_err(|e| e.to_string())?;
        if !output.status.success() { return Err(format!("Could not prepare audio for transcription: {}", String::from_utf8_lossy(&output.stderr))); }
        let mut chunks = std::fs::read_dir(&chunks_dir).map_err(|e| e.to_string())?.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().and_then(|s| s.to_str()) == Some("m4a")).collect::<Vec<_>>();
        chunks.sort(); if chunks.is_empty() { return Err("No audio segments were created".into()); }
        let mut transcript = Vec::new();
        for chunk in chunks { transcript.push(self.transcribe_chunk(chunk, model).await?); }
        Ok(transcript.join("\n\n"))
    }

    async fn summarize(&self, transcript: &str, model: &str, language: &str) -> Result<SummaryOutput, String> {
        let target = if language == "Auto" { "Use the main language of the meeting".to_string() } else { format!("Write the summary, decisions, action items and important details in {language}") };
        let schema = json!({"type":"object","additionalProperties":false,"required":["title","summary","decisions","action_items","important_details"],"properties":{
            "title":{"type":"string"},"summary":{"type":"string"},"decisions":{"type":"array","items":{"type":"string"}},
            "action_items":{"type":"array","items":{"type":"object","additionalProperties":false,"required":["task","owner","deadline"],"properties":{"task":{"type":"string"},"owner":{"type":["string","null"]},"deadline":{"type":["string","null"]}}}},
            "important_details":{"type":"array","items":{"type":"string"}} }});
        let body = json!({"model":model,"messages":[
            {"role":"system","content":format!("You turn meeting transcripts into faithful notes. {target}. Keep transcript quotes in their original language where useful. Use a short 3-8 word title. Record only what participants said. Do not invent people, dates, decisions, deadlines or tasks. If an owner or deadline was not said, use null. Do not omit important agreements.")},
            {"role":"user","content":transcript}],"response_format":{"type":"json_schema","json_schema":{"name":"meeting_notes","strict":true,"schema":schema}}});
        let response = self.client.post(format!("{BASE}/chat/completions")).bearer_auth(&self.key).json(&body).send().await.map_err(|e| e.to_string())?;
        let status = response.status(); let result: Value = response.json().await.map_err(|e| e.to_string())?;
        if !status.is_success() { return Err(format!("Summary request failed (HTTP {}): {}", status.as_u16(), api_error(&result))); }
        let text = result.pointer("/choices/0/message/content").and_then(Value::as_str).ok_or("Summary response contained no content")?;
        serde_json::from_str(text).map_err(|e| format!("Could not read structured summary: {e}"))
    }
}

fn api_error(value: &Value) -> String { value.pointer("/error/message").and_then(Value::as_str).unwrap_or("Unknown API error").chars().take(260).collect() }
