use std::{collections::BTreeMap,path::Path,fs};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::{json,Value};
use tauri::{Manager,ipc::Response};
use crate::{AppState,storage};

fn db(path:&Path)->Result<Connection,String>{
 let c=Connection::open(path).map_err(|e|e.to_string())?;
 c.busy_timeout(std::time::Duration::from_secs(8)).map_err(|e|e.to_string())?;
 c.execute_batch("PRAGMA foreign_keys=ON;").map_err(|e|e.to_string())?;
 Ok(c)
}
pub(crate) fn migrate(path:&Path)->Result<(),String>{
 let c=db(path)?;
 c.execute_batch(
 "CREATE TABLE IF NOT EXISTS collections(name TEXT PRIMARY KEY COLLATE NOCASE, color TEXT NOT NULL DEFAULT '#91B9F7');
  CREATE TABLE IF NOT EXISTS meeting_extras(
    meeting_id TEXT PRIMARY KEY REFERENCES meetings(id) ON DELETE CASCADE,
    collection_name TEXT NOT NULL DEFAULT 'Unsorted',
    bookmarks_json TEXT NOT NULL DEFAULT '[]',
    speaker_names_json TEXT NOT NULL DEFAULT '{}',
    segments_json TEXT NOT NULL DEFAULT '[]',
    translation TEXT,
    detected_mode TEXT
  );
  CREATE TABLE IF NOT EXISTS analysis_cache(
    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
    mode TEXT NOT NULL, signature TEXT NOT NULL, model TEXT NOT NULL, language TEXT NOT NULL,
    result_json TEXT NOT NULL,
    PRIMARY KEY(meeting_id,mode,signature,model,language)
  );
  INSERT OR IGNORE INTO collections(name,color) VALUES('Unsorted','#9CA3AF');
  INSERT OR IGNORE INTO collections(name,color) VALUES('Personal','#91B9F7');
  INSERT OR IGNORE INTO schema_migrations(version) VALUES(2);"
 ).map_err(|e|e.to_string())
}
fn ensure(c:&Connection,id:&str)->Result<(),String>{
 c.execute("INSERT OR IGNORE INTO meeting_extras(meeting_id) VALUES(?1)",[id]).map_err(|e|e.to_string())?;
 Ok(())
}
fn signature(t:&str)->String{
 let mut hash:u64=0xcbf29ce484222325;
 for b in t.as_bytes(){hash ^= *b as u64;hash=hash.wrapping_mul(0x100000001b3);}
 format!("{hash:x}")
}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct Collection {name:String,color:String,count:i64}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct MeetingExtra { collection:String, bookmarks:Vec<f64>,speaker_names:BTreeMap<String,String>,segments:Value,detected_mode:Option<String>,translation:Option<String> }
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
pub(crate) struct ExtrasSnapshot{ collections:Vec<Collection>,meetings:BTreeMap<String,MeetingExtra> }
#[tauri::command]
pub(crate) fn get_extras(state:tauri::State<'_,AppState>)->Result<ExtrasSnapshot,String>{
 let c=db(&state.db_path)?;
 let mut st=c.prepare("SELECT c.name,c.color,(SELECT COUNT(*) FROM meetings m LEFT JOIN meeting_extras e ON e.meeting_id=m.id WHERE COALESCE(e.collection_name,'Unsorted')=c.name) FROM collections c ORDER BY CASE WHEN c.name='Unsorted' THEN 0 ELSE 1 END,c.rowid").map_err(|e|e.to_string())?;
 let collections=st.query_map([],|r|Ok(Collection{name:r.get(0)?,color:r.get(1)?,count:r.get(2)?})).map_err(|e|e.to_string())?.collect::<Result<Vec<_>,_>>().map_err(|e|e.to_string())?;
 let mut stmt=c.prepare("SELECT meeting_id,collection_name,bookmarks_json,speaker_names_json,segments_json,detected_mode,translation FROM meeting_extras").map_err(|e|e.to_string())?;
 let rows=stmt.query_map([],|r|{
   let id:String=r.get(0)?;let name:String=r.get(1)?;let books:String=r.get(2)?;let speakers:String=r.get(3)?;let segs:String=r.get(4)?;
   let mode:Option<String>=r.get(5)?;let translation:Option<String>=r.get(6)?;
   Ok((id,MeetingExtra{collection:name,bookmarks:serde_json::from_str(&books).unwrap_or_default(),speaker_names:serde_json::from_str(&speakers).unwrap_or_default(),segments:serde_json::from_str(&segs).unwrap_or(json!([])),detected_mode:mode,translation}))
 }).map_err(|e|e.to_string())?;
 let meetings=rows.collect::<Result<BTreeMap<_,_>,_>>().map_err(|e|e.to_string())?;
 Ok(ExtrasSnapshot{collections,meetings})
}
fn valid_name(name:&str)->Result<&str,String>{let n=name.trim();if n.is_empty()||n.len()>64 {Err("Enter a collection name (1–64 characters).".into())}else{Ok(n)}}
#[tauri::command]
pub(crate) fn create_collection(state:tauri::State<'_,AppState>,name:String,color:String)->Result<(),String>{
 let n=valid_name(&name)?;let c=db(&state.db_path)?;
 if !(color.len()==7&&color.starts_with('#')&&color[1..].bytes().all(|x|x.is_ascii_hexdigit())){return Err("Invalid collection color".into())}
 c.execute("INSERT INTO collections(name,color) VALUES(?1,?2)",params![n,color]).map_err(|e|e.to_string())?;Ok(())
}
#[tauri::command]
pub(crate) fn rename_collection(state:tauri::State<'_,AppState>,old:String,name:String)->Result<(),String>{
 let n=valid_name(&name)?;if old=="Unsorted" {return Err("Unsorted is a protected collection".into())}
 let mut c=db(&state.db_path)?;let tx=c.transaction().map_err(|e|e.to_string())?;
 tx.execute("UPDATE collections SET name=?1 WHERE name=?2",params![n,old]).map_err(|e|e.to_string())?;
 tx.execute("UPDATE meeting_extras SET collection_name=?1 WHERE collection_name=?2",params![n,old]).map_err(|e|e.to_string())?;
 tx.commit().map_err(|e|e.to_string())
}
#[tauri::command]
pub(crate) fn merge_collection(state:tauri::State<'_,AppState>,source:String,target:String)->Result<(),String>{
 if source==target||source=="Unsorted" {return Err("Choose a different source collection".into())}
 let mut c=db(&state.db_path)?;let tx=c.transaction().map_err(|e|e.to_string())?;
 let exists:i64=tx.query_row("SELECT COUNT(*) FROM collections WHERE name=?1",[&target],|r|r.get(0)).map_err(|e|e.to_string())?;
 if exists==0{return Err("Target collection does not exist".into())}
 tx.execute("UPDATE meeting_extras SET collection_name=?1 WHERE collection_name=?2",params![target,source]).map_err(|e|e.to_string())?;
 tx.execute("DELETE FROM collections WHERE name=?1",[source]).map_err(|e|e.to_string())?;
 tx.commit().map_err(|e|e.to_string())
}
#[tauri::command]
pub(crate) fn delete_collection(state:tauri::State<'_,AppState>,name:String)->Result<(),String>{
 if name=="Unsorted"{return Err("Unsorted is a protected collection".into())}
 let mut c=db(&state.db_path)?;let tx=c.transaction().map_err(|e|e.to_string())?;
 tx.execute("UPDATE meeting_extras SET collection_name='Unsorted' WHERE collection_name=?1",[&name]).map_err(|e|e.to_string())?;
 tx.execute("DELETE FROM collections WHERE name=?1",[&name]).map_err(|e|e.to_string())?;
 tx.commit().map_err(|e|e.to_string())
}
#[tauri::command]
pub(crate) fn assign_collection(state:tauri::State<'_,AppState>,id:String,name:String)->Result<(),String>{
 storage::get(&state.db_path,&id)?;
 let c=db(&state.db_path)?;let exists:i64=c.query_row("SELECT COUNT(*) FROM collections WHERE name=?1",[&name],|r|r.get(0)).map_err(|e|e.to_string())?;
 if exists==0{return Err("Collection not found".into())}
 ensure(&c,&id)?;
 c.execute("UPDATE meeting_extras SET collection_name=?1 WHERE meeting_id=?2",params![name,id]).map_err(|e|e.to_string())?;Ok(())
}
#[tauri::command]
pub(crate) fn toggle_bookmark(state:tauri::State<'_,AppState>,id:String,time:f64)->Result<(),String>{
 if !time.is_finite()||time<0.0{return Err("Invalid timestamp".into())}
 storage::get(&state.db_path,&id)?;let c=db(&state.db_path)?;ensure(&c,&id)?;
 let text:String=c.query_row("SELECT bookmarks_json FROM meeting_extras WHERE meeting_id=?1",[&id],|r|r.get(0)).map_err(|e|e.to_string())?;
 let mut list:Vec<f64>=serde_json::from_str(&text).unwrap_or_default();
 if list.iter().any(|x|(x-time).abs()<0.05){list.retain(|x|(x-time).abs()>=0.05)}else{list.push(time);list.sort_by(f64::total_cmp)}
 c.execute("UPDATE meeting_extras SET bookmarks_json=?1 WHERE meeting_id=?2",params![serde_json::to_string(&list).unwrap_or_default(),id]).map_err(|e|e.to_string())?;Ok(())
}
#[tauri::command]
pub(crate) fn rename_speaker(state:tauri::State<'_,AppState>,id:String,speaker:String,name:String)->Result<(),String>{
 if name.trim().is_empty()||name.len()>60{return Err("Enter a shorter speaker name".into())}
 storage::get(&state.db_path,&id)?;let c=db(&state.db_path)?;ensure(&c,&id)?;
 let text:String=c.query_row("SELECT speaker_names_json FROM meeting_extras WHERE meeting_id=?1",[&id],|r|r.get(0)).map_err(|e|e.to_string())?;
 let mut names:BTreeMap<String,String>=serde_json::from_str(&text).unwrap_or_default();names.insert(speaker,name.trim().into());
 c.execute("UPDATE meeting_extras SET speaker_names_json=?1 WHERE meeting_id=?2",params![serde_json::to_string(&names).unwrap_or_default(),id]).map_err(|e|e.to_string())?;Ok(())
}
pub(crate) fn save_segments(path:&Path,id:&str,segments:&Value)->Result<(),String>{
 let c=db(path)?;ensure(&c,id)?;
 c.execute("UPDATE meeting_extras SET segments_json=?1 WHERE meeting_id=?2",params![segments.to_string(),id]).map_err(|e|e.to_string())?;Ok(())
}
fn parse_response(v:&Value)->Result<Value,String>{
 let text=v.pointer("/choices/0/message/content").and_then(Value::as_str).ok_or("AI returned no content")?;
 serde_json::from_str(text).map_err(|e|format!("AI returned invalid JSON: {e}"))
}
fn api_error(v:&Value)->String {v.pointer("/error/message").and_then(Value::as_str).unwrap_or("AI request failed").chars().take(350).collect()}
async fn chat(key:&str,model:&str,system:String,user:String)->Result<Value,String>{
 let response=reqwest::Client::new().post("https://api.openai.com/v1/chat/completions").bearer_auth(key)
 .json(&json!({"model":model,"messages":[{"role":"system","content":system},{"role":"user","content":user}],"response_format":{"type":"json_object"}}))
 .send().await.map_err(|e|e.to_string())?;
 let status=response.status();let data:Value=response.json().await.map_err(|e|e.to_string())?;
 if !status.is_success(){return Err(format!("AI HTTP {}: {}",status.as_u16(),api_error(&data)))}parse_response(&data)
}
fn cache_key(path:&Path,id:&str,_mode:&str)->Result<(String,String,String),String>{
 let m=storage::get(path,id)?;let s=storage::load_settings(path)?;
 Ok((signature(m.transcript.as_deref().unwrap_or("")),s.summary_model,s.summary_language))
}
// Opening a meeting must not submit its transcript or write generated notes.
#[tauri::command]
pub(crate) fn cached_analysis(state:tauri::State<'_,AppState>,id:String)->Result<Option<Value>,String>{
 let (sig,model,language)=cache_key(&state.db_path,&id,"Auto")?;
 let c=db(&state.db_path)?;
 let result:Option<String>=c.query_row(
   "SELECT a.result_json FROM analysis_cache a JOIN meeting_extras e ON e.meeting_id=a.meeting_id AND e.detected_mode=a.mode WHERE a.meeting_id=?1 AND a.signature=?2 AND a.model=?3 AND a.language=?4",
   params![id,sig,model,language],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
 result.map(|v|serde_json::from_str(&v).map_err(|e|e.to_string())).transpose()
}
#[tauri::command]
pub(crate) async fn analyze_meeting(app:tauri::AppHandle,id:String,mode:String,force:bool)->Result<Value,String>{
 let state=app.state::<AppState>();let path=state.db_path.clone();
 let m=storage::get(&path,&id)?;let transcript=m.transcript.clone().ok_or("Transcript is not ready yet")?;
 if transcript.trim().is_empty(){return Err("Transcript is empty".into())}
 let mode=match mode.as_str(){"Auto"|"Meeting"|"Lecture"|"Interview"|"Video"|"Custom"=>mode,_=>return Err("Invalid analysis type".into())};
 let (sig,model,language)=cache_key(&path,&id,&mode)?;
 if !force {
   let c=db(&path)?;
   if mode=="Auto"{
     let previous:Option<String>=c.query_row("SELECT detected_mode FROM meeting_extras WHERE meeting_id=?1",[&id],|r|r.get(0)).optional().map_err(|e|e.to_string())?.flatten();
     if let Some(detected)=previous {let result:Option<String>=c.query_row("SELECT result_json FROM analysis_cache WHERE meeting_id=?1 AND mode=?2 AND signature=?3 AND model=?4 AND language=?5",params![id,detected,sig,model,language],|r|r.get(0)).optional().map_err(|e|e.to_string())?;if let Some(r)=result{return serde_json::from_str(&r).map_err(|e|e.to_string())}}
   }else{
    let result:Option<String>=c.query_row("SELECT result_json FROM analysis_cache WHERE meeting_id=?1 AND mode=?2 AND signature=?3 AND model=?4 AND language=?5",params![id,mode,sig,model,language],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
    if let Some(result)=result{return serde_json::from_str(&result).map_err(|e|e.to_string())}
   }
 }
 let secret=crate::key().ok_or("Add your OpenAI API key in Settings")?;
 let chosen=if mode=="Auto"{"Determine whether the transcript is Meeting, Lecture, Interview or Video. Use the most appropriate type." .to_string()}else{format!("The analysis type is {mode}. Do not change it.")};
 let system=format!("Analyze ONLY the provided transcript. {chosen} Write in {language} (or use the source language if Auto). Return a JSON object with exactly these keys: mode (Meeting, Lecture, Interview, Video, or Custom), title, summary (paragraph), decisions (array of important decisions or key points), details (array of key facts), actionItems (array of objects with task, owner, deadline; each may be null). Do not invent facts, people or tasks. A lecture should produce lecture notes and learning takeaways, an interview candidate responses and observations, a video key takeaways, and a meeting decisions and actions. Return only JSON.");
 let excerpt=if transcript.chars().count()>56000 {format!("{}\n[Long transcript shortened]\n{}",transcript.chars().take(47000).collect::<String>(),transcript.chars().rev().take(9000).collect::<String>().chars().rev().collect::<String>())}else{transcript};
 let mut result=chat(&secret,&model,system,excerpt).await?;
 let actual=if mode=="Auto"{result.get("mode").and_then(Value::as_str).unwrap_or("Meeting").to_string()}else{mode.clone()};
 let actual=if ["Meeting","Lecture","Interview","Video","Custom"].contains(&actual.as_str()){actual}else{"Meeting".into()};
 result["mode"]=json!(actual);
 let c=db(&path)?;ensure(&c,&id)?;
 c.execute("INSERT INTO analysis_cache(meeting_id,mode,signature,model,language,result_json) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(meeting_id,mode,signature,model,language) DO UPDATE SET result_json=excluded.result_json",params![id,actual,sig,model,language,result.to_string()]).map_err(|e|e.to_string())?;
 if mode=="Auto" {c.execute("UPDATE meeting_extras SET detected_mode=?1 WHERE meeting_id=?2",params![actual,id]).map_err(|e|e.to_string())?;}
 Ok(result)
}
#[tauri::command]
pub(crate) async fn ask_meeting(app:tauri::AppHandle,id:String,question:String)->Result<Value,String>{
 if question.trim().is_empty()||question.len()>2000{return Err("Question must be 1–2000 characters".into())}
 let path=app.state::<AppState>().db_path.clone();let m=storage::get(&path,&id)?;
 let transcript=m.transcript.ok_or("Transcript is not ready")?;
 let settings=storage::load_settings(&path)?;let secret=crate::key().ok_or("Add your OpenAI API key in Settings")?;
 let content=if transcript.len()>70000{transcript.chars().take(70000).collect::<String>()}else{transcript.clone()};
 let system=format!("Answer only questions about this meeting using the transcript below. Never use external knowledge. If the answer is missing say so. Return JSON object with 'answer' (short string) and 'quote' (exact verbatim relevant excerpt from transcript, or empty string if none). If possible include only direct evidence in quote. Transcript:\n{content}");
 let mut response=chat(&secret,&settings.summary_model,system,question).await?;
 let quote=response.get("quote").and_then(Value::as_str).unwrap_or("");
 if !quote.is_empty()&&!transcript.contains(quote){response["quote"]=json!("")}
 Ok(response)
}
#[tauri::command]
pub(crate) async fn translate_meeting(app:tauri::AppHandle,id:String,language:String)->Result<String,String>{
 if !["Russian","English","Turkish","Tajik","German","French"].contains(&language.as_str()){return Err("Unsupported translation language".into())}
 let path=app.state::<AppState>().db_path.clone();let m=storage::get(&path,&id)?;
 let transcript=m.transcript.ok_or("Transcript is not ready")?;let c=db(&path)?;ensure(&c,&id)?;
 let key=format!("translation_{language}");
 let cached:Option<String>=c.query_row("SELECT translation FROM meeting_extras WHERE meeting_id=?1",[&id],|r|r.get(0)).optional().map_err(|e|e.to_string())?.flatten();
 if let Some(v)=cached.as_ref(){if let Ok(v)=serde_json::from_str::<Value>(v){if let Some(t)=v.get(&key).and_then(Value::as_str){return Ok(t.to_string())}}}
 let settings=storage::load_settings(&path)?;let secret=crate::key().ok_or("Add your OpenAI API key in Settings")?;
 let result=chat(&secret,&settings.summary_model,format!("Translate the entire provided transcript to {language}. Preserve paragraph breaks and meaning. Do not invent content. Respond JSON with exactly a field 'translation' containing translated text."),transcript).await?;
 let translation=result.get("translation").and_then(Value::as_str).ok_or("Translation response missing text")?.to_string();
 let mut map=cached.as_deref().and_then(|v|serde_json::from_str::<Value>(v).ok()).unwrap_or(json!({}));
 map[key.as_str()]=json!(translation);
 c.execute("UPDATE meeting_extras SET translation=?1 WHERE meeting_id=?2",params![map.to_string(),id]).map_err(|e|e.to_string())?;
 Ok(translation)
}
#[tauri::command]
pub(crate) fn recording_bytes(state:tauri::State<'_,AppState>,id:String)->Result<Response,String>{
 let m=storage::get(&state.db_path,&id)?;let path=Path::new(&m.audio_path);
 let meta=fs::metadata(path).map_err(|e|e.to_string())?;
 if meta.len()>100*1024*1024 {return Err("This recording is too large for in-app playback. Use Open recording instead.".into())}
 fs::read(path).map(Response::new).map_err(|e|e.to_string())
}

#[tauri::command]
pub(crate) fn save_export(name:String,extension:String,bytes:Vec<u8>)->Result<Option<String>,String>{
 let ext=extension.as_str();
 if !["md","docx"].contains(&ext){return Err("Unsupported export format".into())}
 let suggested: String=name.chars().filter(|c|!['/','\\',':','*','?','"','<','>','|'].contains(c)).take(85).collect();
 let filename=format!("{}.{}",if suggested.trim().is_empty(){"Meeting Notes"}else{suggested.trim()},ext);
 let chosen=rfd::FileDialog::new().add_filter(if ext=="md"{"Markdown"}else{"Word document"},&[ext]).set_file_name(&filename).save_file();
 if let Some(path)=chosen{
   fs::write(&path,bytes).map_err(|e|e.to_string())?;
   Ok(Some(path.to_string_lossy().to_string()))
 }else{Ok(None)}
}
