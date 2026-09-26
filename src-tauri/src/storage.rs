use std::path::Path;
use rusqlite::{params, Connection};
use crate::models::{ActionItem, Meeting, SearchResult, Settings};

fn conn(path: &Path) -> Result<Connection, String> {
    let c = Connection::open(path).map_err(|e| e.to_string())?;
    c.busy_timeout(std::time::Duration::from_secs(5)).map_err(|e| e.to_string())?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;").map_err(|e| e.to_string())?;
    Ok(c)
}

pub fn migrate(path: &Path) -> Result<(), String> {
    let c = conn(path)?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS schema_migrations(version INTEGER PRIMARY KEY);
      CREATE TABLE IF NOT EXISTS meetings(
        id TEXT PRIMARY KEY, generated_title TEXT NOT NULL, custom_title TEXT,
        created_at TEXT NOT NULL, started_at TEXT NOT NULL, ended_at TEXT NOT NULL,
        duration_seconds INTEGER NOT NULL, audio_path TEXT NOT NULL, transcript TEXT,
        summary TEXT, decisions_json TEXT NOT NULL DEFAULT '[]', details_json TEXT NOT NULL DEFAULT '[]',
        status TEXT NOT NULL, error TEXT, transcription_model TEXT NOT NULL, summary_model TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS action_items(
        id TEXT PRIMARY KEY, meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
        task TEXT NOT NULL, owner TEXT, deadline TEXT, completed INTEGER NOT NULL DEFAULT 0);
      CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
      CREATE VIRTUAL TABLE IF NOT EXISTS meeting_fts USING fts5(
        meeting_id UNINDEXED, title, summary, decisions, action_items, important_details, transcript);
      INSERT OR IGNORE INTO schema_migrations(version) VALUES(1);")
      .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn load_settings(path: &Path) -> Result<Settings, String> {
    let c = conn(path)?;
    let text: Option<String> = c.query_row("SELECT value FROM settings WHERE key='app'", [], |row| row.get(0)).ok();
    match text { Some(value) => serde_json::from_str(&value).map_err(|e| e.to_string()), None => Ok(Settings::default()) }
}

pub fn save_settings(path: &Path, settings: &Settings) -> Result<(), String> {
    let c = conn(path)?;
    c.execute("INSERT INTO settings(key,value) VALUES('app',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [serde_json::to_string(settings).map_err(|e| e.to_string())?]).map_err(|e| e.to_string())?;
    Ok(())
}

fn load_actions(c: &Connection, meeting_id: &str) -> Result<Vec<ActionItem>, String> {
    let mut stmt = c.prepare("SELECT id,task,owner,deadline,completed FROM action_items WHERE meeting_id=?1 ORDER BY rowid").map_err(|e| e.to_string())?;
    let rows = stmt.query_map([meeting_id], |r| Ok(ActionItem { id: r.get(0)?, task: r.get(1)?, owner: r.get(2)?, deadline: r.get(3)?, completed: r.get::<_, i64>(4)? != 0 })).map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>,_>>().map_err(|e| e.to_string())
}

fn load_one(c: &Connection, id: &str) -> Result<Meeting, String> {
    let mut meeting = c.query_row("SELECT id,generated_title,custom_title,created_at,started_at,ended_at,duration_seconds,audio_path,transcript,summary,decisions_json,details_json,status,error,transcription_model,summary_model FROM meetings WHERE id=?1", [id], |r| {
        let decisions: String = r.get(10)?; let details: String = r.get(11)?;
        Ok(Meeting { id:r.get(0)?, generated_title:r.get(1)?, custom_title:r.get(2)?, created_at:r.get(3)?, started_at:r.get(4)?, ended_at:r.get(5)?, duration_seconds:r.get(6)?, audio_path:r.get(7)?, transcript:r.get(8)?, summary:r.get(9)?, decisions:serde_json::from_str(&decisions).unwrap_or_default(), action_items:Vec::new(), important_details:serde_json::from_str(&details).unwrap_or_default(), status:r.get(12)?, error:r.get(13)?, transcription_model:r.get(14)?, summary_model:r.get(15)? })
    }).map_err(|e| e.to_string())?;
    meeting.action_items = load_actions(c, id)?;
    Ok(meeting)
}

pub fn get(path: &Path, id: &str) -> Result<Meeting, String> { load_one(&conn(path)?, id) }

pub fn list(path: &Path) -> Result<Vec<Meeting>, String> {
    let c = conn(path)?;
    let mut stmt = c.prepare("SELECT id FROM meetings ORDER BY started_at DESC").map_err(|e| e.to_string())?;
    let ids = stmt.query_map([], |r| r.get::<_,String>(0)).map_err(|e| e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e| e.to_string())?;
    ids.iter().map(|id| load_one(&c, id)).collect()
}

pub fn save(path: &Path, m: &Meeting) -> Result<(), String> {
    let mut c = conn(path)?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO meetings(id,generated_title,custom_title,created_at,started_at,ended_at,duration_seconds,audio_path,transcript,summary,decisions_json,details_json,status,error,transcription_model,summary_model)
      VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)
      ON CONFLICT(id) DO UPDATE SET generated_title=excluded.generated_title,
      audio_path=excluded.audio_path,transcript=excluded.transcript,summary=excluded.summary,
      decisions_json=excluded.decisions_json,details_json=excluded.details_json,status=excluded.status,
      error=excluded.error,transcription_model=excluded.transcription_model,summary_model=excluded.summary_model",
      params![m.id,m.generated_title,m.custom_title,m.created_at,m.started_at,m.ended_at,m.duration_seconds,m.audio_path,m.transcript,m.summary,serde_json::to_string(&m.decisions).unwrap(),serde_json::to_string(&m.important_details).unwrap(),m.status,m.error,m.transcription_model,m.summary_model]).map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM action_items WHERE meeting_id=?1", [&m.id]).map_err(|e| e.to_string())?;
    for item in &m.action_items { tx.execute("INSERT INTO action_items(id,meeting_id,task,owner,deadline,completed) VALUES(?1,?2,?3,?4,?5,?6)", params![item.id,m.id,item.task,item.owner,item.deadline,item.completed as i64]).map_err(|e| e.to_string())?; }
    let title: String = tx.query_row("SELECT COALESCE(custom_title,generated_title) FROM meetings WHERE id=?1",[&m.id],|row|row.get(0)).map_err(|e|e.to_string())?;
    tx.execute("DELETE FROM meeting_fts WHERE meeting_id=?1", [&m.id]).map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO meeting_fts(meeting_id,title,summary,decisions,action_items,important_details,transcript) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![m.id,title,m.summary.as_deref().unwrap_or(""),m.decisions.join(" "),m.action_items.iter().map(|a| a.task.as_str()).collect::<Vec<_>>().join(" "),m.important_details.join(" "),m.transcript.as_deref().unwrap_or("")]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub fn rename_title(path: &Path, id: &str, title: &str) -> Result<(), String> {
    let mut c = conn(path)?; let tx = c.transaction().map_err(|e|e.to_string())?;
    let changed = tx.execute("UPDATE meetings SET custom_title=?2 WHERE id=?1", params![id,title]).map_err(|e|e.to_string())?;
    if changed == 0 { return Err("Meeting not found".into()); }
    tx.execute("UPDATE meeting_fts SET title=?2 WHERE meeting_id=?1",params![id,title]).map_err(|e|e.to_string())?;
    tx.commit().map_err(|e|e.to_string())
}

pub fn toggle_action(path: &Path, id: &str, completed: bool) -> Result<(), String> {
    let c = conn(path)?;
    c.execute("UPDATE action_items SET completed=?2 WHERE id=?1", params![id,completed as i64]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete(path: &Path, id: &str) -> Result<(), String> {
    let mut c = conn(path)?; let tx = c.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM meeting_fts WHERE meeting_id=?1", [id]).map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM meetings WHERE id=?1", [id]).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

pub fn search(path: &Path, query: &str, source: &str) -> Result<Vec<SearchResult>, String> {
    let c = conn(path)?;
    let phrase = format!("\"{}\"", query.replace('"', "\"\""));
    let mut stmt = c.prepare("SELECT meeting_id FROM meeting_fts WHERE meeting_fts MATCH ?1 LIMIT 100").map_err(|e| e.to_string())?;
    let ids = stmt.query_map([phrase], |r| r.get::<_,String>(0)).map_err(|e| e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    for id in ids { let m = load_one(&c, &id)?; let fields = [ ("Summary", m.summary.clone().unwrap_or_default()), ("Action item", m.action_items.iter().map(|a| a.task.clone()).collect::<Vec<_>>().join(" ")), ("Transcript", m.transcript.clone().unwrap_or_default()), ("Decision", m.decisions.join(" ")), ("Important detail", m.important_details.join(" ")) ];
      for (label, text) in fields { if source != "all" && !label.to_lowercase().starts_with(source) { continue; } if text.to_lowercase().contains(&query.to_lowercase()) { let part = text.split_inclusive(['.', '\n', '?', '!']).find(|segment| segment.to_lowercase().contains(&query.to_lowercase())).unwrap_or(&text); let snippet = part.chars().take(240).collect(); results.push(SearchResult { meeting_id:id.clone(),title:m.title().into(),started_at:m.started_at.clone(),source:label.into(),snippet }); } }
      if m.title().to_lowercase().contains(&query.to_lowercase()) && source == "all" && results.iter().all(|r| r.meeting_id != id) { results.push(SearchResult { meeting_id:id,title:m.title().into(),started_at:m.started_at,source:"Title".into(),snippet:m.summary.unwrap_or_default() }); }
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retains_user_title_and_finds_multilingual_notes() {
        let directory = std::env::temp_dir().join(format!("meeting-notes-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let database = directory.join("test.db"); migrate(&database).unwrap();
        let meeting = Meeting { id:"meeting-1".into(), generated_title:"Planning call".into(), custom_title:Some("План запуска".into()), created_at:"2026-09-25T10:00:00Z".into(), started_at:"2026-09-25T10:00:00Z".into(), ended_at:"2026-09-25T10:30:00Z".into(), duration_seconds:1800, audio_path:"recording.m4a".into(), transcript:Some("We discussed onboarding with the team.".into()), summary:Some("Команда согласовала запуск продукта.".into()), decisions:vec!["Запуск в октябре".into()], action_items:vec![], important_details:vec![], status:"READY".into(), error:None, transcription_model:"gpt-4o-mini-transcribe".into(), summary_model:"gpt-4o-mini".into() };
        save(&database,&meeting).unwrap();
        assert_eq!(get(&database,"meeting-1").unwrap().custom_title.as_deref(),Some("План запуска"));
        assert!(!search(&database,"запуск","all").unwrap().is_empty());
        assert!(!search(&database,"onboarding","transcript").unwrap().is_empty());
        rename_title(&database,"meeting-1","Мой новый заголовок").unwrap();
        save(&database,&meeting).unwrap();
        assert_eq!(get(&database,"meeting-1").unwrap().title(),"Мой новый заголовок");
        assert!(!search(&database,"заголовок","all").unwrap().is_empty());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
