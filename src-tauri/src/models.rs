use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionItem {
    pub id: String,
    pub task: String,
    pub owner: Option<String>,
    pub deadline: Option<String>,
    pub completed: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meeting {
    pub id: String,
    pub generated_title: String,
    pub custom_title: Option<String>,
    pub created_at: String,
    pub started_at: String,
    pub ended_at: String,
    pub duration_seconds: i64,
    pub audio_path: String,
    pub transcript: Option<String>,
    pub summary: Option<String>,
    pub decisions: Vec<String>,
    pub action_items: Vec<ActionItem>,
    pub important_details: Vec<String>,
    pub status: String,
    pub error: Option<String>,
    pub transcription_model: String,
    pub summary_model: String,
}

impl Meeting {
    pub fn title(&self) -> &str {
        self.custom_title.as_deref().unwrap_or(&self.generated_title)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub theme: String,
    pub language: String,
    pub launch_at_login: bool,
    pub minimize_to_tray: bool,
    pub microphone_id: String,
    pub system_audio_id: String,
    pub recording_path: String,
    pub transcription_model: String,
    pub summary_model: String,
    pub summary_language: String,
    pub shortcut: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(), language: "en".into(), launch_at_login: false,
            minimize_to_tray: true, microphone_id: String::new(), system_audio_id: String::new(),
            recording_path: String::new(), transcription_model: "gpt-4o-mini-transcribe".into(),
            summary_model: "gpt-4o-mini".into(), summary_language: "Russian".into(),
            shortcut: "Ctrl+Shift+R".into(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub meetings: Vec<Meeting>,
    pub settings: Settings,
    pub key_connected: bool,
    pub recording_started_at: Option<String>,
    pub microphones: Vec<Device>,
    pub outputs: Vec<Device>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub meeting_id: String,
    pub title: String,
    pub started_at: String,
    pub source: String,
    pub snippet: String,
}

#[derive(Deserialize)]
pub struct SummaryOutput {
    pub title: String,
    pub summary: String,
    pub decisions: Vec<String>,
    pub action_items: Vec<SummaryAction>,
    pub important_details: Vec<String>,
}

#[derive(Deserialize)]
pub struct SummaryAction {
    pub task: String,
    pub owner: Option<String>,
    pub deadline: Option<String>,
}
