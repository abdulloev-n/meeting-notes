mod ai;
mod audio;
mod models;
mod storage;

use std::{path::PathBuf, sync::{Mutex, atomic::{AtomicBool, Ordering}}};
use chrono::Utc;
use tauri::{Manager, Emitter};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_autostart::ManagerExt as AutostartExt;
use uuid::Uuid;
use ai::{AIProvider, OpenAIProvider};
use models::{ActionItem, Meeting, SearchResult, Settings, Snapshot};

struct AppState { db_path: PathBuf, default_recordings: PathBuf, ffmpeg_path: PathBuf, recording: Mutex<Option<audio::Session>>, starting: AtomicBool }
fn credential() -> Result<keyring::Entry,String> { keyring::Entry::new("Meeting Notes","OpenAI API key").map_err(|e| e.to_string()) }
fn key() -> Option<String> { credential().ok()?.get_password().ok() }
fn root(state: &AppState, s: &Settings) -> PathBuf { if s.recording_path.is_empty() { state.default_recordings.clone() } else { PathBuf::from(&s.recording_path) } }
fn updated(app: &tauri::AppHandle) { let _ = app.emit("meeting-updated", ()); }

#[tauri::command]
fn get_snapshot(state: tauri::State<'_,AppState>) -> Result<Snapshot,String> {
    Ok(Snapshot { meetings:storage::list(&state.db_path)?, settings:storage::load_settings(&state.db_path)?, key_connected:key().is_some(),
        recording_started_at:state.recording.lock().map_err(|e| e.to_string())?.as_ref().map(|s| s.started_at.to_rfc3339()),
        microphones:audio::microphones(), outputs:audio::outputs() })
}

async fn start_impl(app: tauri::AppHandle) -> Result<(),String> {
    let state = app.state::<AppState>();
    if state.starting.swap(true, Ordering::SeqCst) { return Err("Recording is starting".into()); }
    let result = async {
        if state.recording.lock().map_err(|e| e.to_string())?.is_some() { return Err("A recording is in progress".into()); }
        let settings = storage::load_settings(&state.db_path)?;
        let directory = root(&state,&settings).join(format!("{}_{}",Utc::now().format("%Y-%m-%d_%H-%M-%S"),Uuid::new_v4()));
        let session = tauri::async_runtime::spawn_blocking(move || audio::start(directory,settings.microphone_id,settings.system_audio_id)).await.map_err(|e| e.to_string())??;
        let mut guard = state.recording.lock().map_err(|e| e.to_string())?;
        *guard = Some(session); drop(guard);
        let _ = app.emit("recording-changed", ()); refresh_tray(&app); Ok(())
    }.await;
    state.starting.store(false, Ordering::SeqCst);
    result
}
#[tauri::command]
async fn start_recording(app: tauri::AppHandle) -> Result<(),String> { start_impl(app).await }

async fn stop_impl(app: tauri::AppHandle) -> Result<Meeting,String> {
    let state = app.state::<AppState>();
    let session = state.recording.lock().map_err(|e| e.to_string())?.take().ok_or("No active recording")?;
    let started = session.started_at; let directory = session.directory.clone(); let ffmpeg = state.ffmpeg_path.clone();
    let result = tauri::async_runtime::spawn_blocking(move || session.finish(&ffmpeg)).await.map_err(|e| e.to_string())?;
    let (ended,path,failure) = match result { Ok((time,path)) => (time,path,None), Err(error) => {
        log::error!("Recording finalization: {error}"); let fallback = audio::usable_raw_audio(&directory)
            .ok_or_else(|| format!("Recording could not be saved: {error}. Check {} for raw audio files.",directory.display()))?;
        (Utc::now(),fallback,Some(error)) } };
    let settings = storage::load_settings(&state.db_path)?;
    let meeting = Meeting { id:Uuid::new_v4().to_string(), generated_title:format!("Meeting {}",started.format("%b %d, %Y")), custom_title:None,
        created_at:started.to_rfc3339(), started_at:started.to_rfc3339(), ended_at:ended.to_rfc3339(), duration_seconds:(ended-started).num_seconds().max(0),
        audio_path:path.to_string_lossy().into_owned(), transcript:None, summary:None, decisions:vec![],action_items:vec![],important_details:vec![],
        status:if failure.is_some() {"TRANSCRIPTION_FAILED"} else {"RECORDED"}.into(), error:failure,
        transcription_model:settings.transcription_model, summary_model:settings.summary_model };
    storage::save(&state.db_path,&meeting)?; updated(&app); let _ = app.emit("recording-changed", ()); refresh_tray(&app);
    if meeting.status == "RECORDED" { let id = meeting.id.clone(); tauri::async_runtime::spawn(async move { process_meeting(app,id).await; }); }
    Ok(meeting)
}
#[tauri::command]
async fn stop_recording(app: tauri::AppHandle) -> Result<Meeting,String> { stop_impl(app).await }

fn set_status(app: &tauri::AppHandle,id: &str,status: &str,error: Option<String>) -> Result<Meeting,String> {
    let state = app.state::<AppState>(); let mut m = storage::get(&state.db_path,id)?; m.status=status.into(); m.error=error;
    storage::save(&state.db_path,&m)?; updated(app); Ok(m)
}
async fn process_meeting(app: tauri::AppHandle,id: String) {
    let state = app.state::<AppState>();
    let Some(secret) = key() else { let _ = set_status(&app,&id,"TRANSCRIPTION_FAILED",Some("Add your OpenAI API key in Settings, then retry processing.".into())); return; };
    let provider = OpenAIProvider::new(secret);
    let mut m = match storage::get(&state.db_path,&id) { Ok(m) => m,Err(e) => { log::error!("Load meeting: {e}"); return; } };
    if m.transcript.is_none() {
        if std::path::Path::new(&m.audio_path).extension().and_then(|extension|extension.to_str()) != Some("m4a") {
            let directory = std::path::Path::new(&m.audio_path).parent().map(std::path::Path::to_path_buf);
            let ffmpeg = state.ffmpeg_path.clone();
            let result = match directory { Some(directory) => tauri::async_runtime::spawn_blocking(move || audio::mix_recording(&directory,&ffmpeg)).await.map_err(|e|e.to_string()).and_then(|value|value), None => Err("Recording folder missing".into()) };
            match result { Ok(path) => { m.audio_path=path.to_string_lossy().into_owned(); if let Err(e)=storage::save(&state.db_path,&m){log::error!("Save recovered audio path: {e}");return;} },
                Err(error) => {let _=set_status(&app,&id,"TRANSCRIPTION_FAILED",Some(format!("Audio conversion failed: {error}. The original WAV file remains on disk.")));return;} }
        }
        if let Err(e)=set_status(&app,&id,"TRANSCRIBING",None) { log::error!("Save status: {e}"); return; }
        match provider.transcribe(std::path::Path::new(&m.audio_path),&state.ffmpeg_path,&m.transcription_model).await {
            Ok(transcript) => { m.transcript=Some(transcript); m.status="TRANSCRIBED".into(); m.error=None; if let Err(e)=storage::save(&state.db_path,&m) { log::error!("Save transcript: {e}"); return; } updated(&app); },
            Err(error) => { log::error!("Transcription failed: {error}"); let _=set_status(&app,&id,"TRANSCRIPTION_FAILED",Some(error)); return; }
        }
    }
    if let Err(e)=set_status(&app,&id,"SUMMARIZING",None) { log::error!("Save status: {e}"); return; }
    let settings=match storage::load_settings(&state.db_path) { Ok(s)=>s,Err(e)=>{log::error!("Load settings: {e}");return;} };
    match provider.summarize(m.transcript.as_deref().unwrap_or(""),&m.summary_model,&settings.summary_language).await {
        Ok(output) => { m.generated_title=output.title; m.summary=Some(output.summary); m.decisions=output.decisions; m.important_details=output.important_details;
            m.action_items=output.action_items.into_iter().map(|a| ActionItem{id:Uuid::new_v4().to_string(),task:a.task,owner:a.owner,deadline:a.deadline,completed:false}).collect();
            m.status="READY".into(); m.error=None;
            if let Ok(current)=storage::get(&state.db_path,&id) { m.custom_title=current.custom_title; }
            if let Err(e)=storage::save(&state.db_path,&m) { log::error!("Save summary: {e}"); return; } updated(&app);
        }, Err(error) => { log::error!("Summary failed: {error}"); let _=set_status(&app,&id,"SUMMARY_FAILED",Some(error)); }
    }
}

#[tauri::command]
fn rename_meeting(app:tauri::AppHandle,state:tauri::State<'_,AppState>,id:String,title:String)->Result<(),String>{
    if title.trim().is_empty(){return Err("Enter a meeting title".into());}
    storage::rename_title(&state.db_path,&id,title.trim())?;updated(&app);Ok(())
}
#[tauri::command]
fn toggle_action(app:tauri::AppHandle,state:tauri::State<'_,AppState>,id:String,completed:bool)->Result<(),String>{storage::toggle_action(&state.db_path,&id,completed)?;updated(&app);Ok(())}
#[tauri::command]
fn delete_meeting(app:tauri::AppHandle,state:tauri::State<'_,AppState>,id:String)->Result<(),String>{
    let m=storage::get(&state.db_path,&id)?;storage::delete(&state.db_path,&id)?;
    if let Some(parent)=std::path::Path::new(&m.audio_path).parent(){if let Err(e)=std::fs::remove_dir_all(parent){log::error!("Delete audio folder: {e}");}}
    updated(&app);Ok(())
}
#[tauri::command]
fn search_meetings(state:tauri::State<'_,AppState>,query:String,source:String)->Result<Vec<SearchResult>,String>{if query.trim().is_empty(){Ok(vec![])}else{storage::search(&state.db_path,query.trim(),&source)}}
#[tauri::command]
fn save_settings(app:tauri::AppHandle,state:tauri::State<'_,AppState>,settings:Settings)->Result<(),String>{
    let old=storage::load_settings(&state.db_path)?;
    if old.shortcut!=settings.shortcut { app.global_shortcut().unregister(old.shortcut.as_str()).map_err(|e|e.to_string())?;
        if let Err(e)=app.global_shortcut().register(settings.shortcut.as_str()){let _=app.global_shortcut().register(old.shortcut.as_str());return Err(format!("Shortcut unavailable: {e}"));} }
    if old.launch_at_login!=settings.launch_at_login { let result=if settings.launch_at_login{app.autolaunch().enable()}else{app.autolaunch().disable()};result.map_err(|e|e.to_string())?; }
    storage::save_settings(&state.db_path,&settings)?;let _=app.emit("settings-updated",());Ok(())
}
#[tauri::command]
async fn save_api_key(key:String)->Result<(),String>{let value=key.trim();if value.is_empty(){return Err("Enter an API key".into());}OpenAIProvider::test_key(value).await?;credential()?.set_password(value).map_err(|e|e.to_string())}
#[tauri::command]
fn retry_processing(app:tauri::AppHandle,state:tauri::State<'_,AppState>,id:String)->Result<(),String>{let m=storage::get(&state.db_path,&id)?;
    if !["RECORDED","TRANSCRIPTION_FAILED","SUMMARY_FAILED"].contains(&m.status.as_str()){return Err("Meeting is processing or complete".into());}
    tauri::async_runtime::spawn(async move{process_meeting(app,id).await;});Ok(())}
#[tauri::command]
fn choose_storage()->Option<String>{rfd::FileDialog::new().pick_folder().map(|p|p.to_string_lossy().into_owned())}
#[tauri::command]
fn open_storage(state:tauri::State<'_,AppState>)->Result<(),String>{let s=storage::load_settings(&state.db_path)?;let path=root(&state,&s);std::fs::create_dir_all(&path).map_err(|e|e.to_string())?;open::that(path).map_err(|e|e.to_string())}
#[tauri::command]
fn open_audio(state:tauri::State<'_,AppState>,id:String)->Result<(),String>{let m=storage::get(&state.db_path,&id)?;open::that(m.audio_path).map_err(|e|e.to_string())}

fn refresh_tray(app:&tauri::AppHandle){
    let active=app.state::<AppState>().recording.lock().map(|g|g.is_some()).unwrap_or(false);
    let items=(MenuItem::with_id(app,"toggle",if active{"Stop recording"}else{"Start recording"},true,None::<&str>),MenuItem::with_id(app,"open","Open app",true,None::<&str>),PredefinedMenuItem::separator(app),MenuItem::with_id(app,"quit","Quit",true,None::<&str>));
    if let (Ok(toggle),Ok(open),Ok(sep),Ok(quit))=items{if let Ok(menu)=Menu::with_items(app,&[&toggle,&open,&sep,&quit]){if let Some(tray)=app.tray_by_id("main"){let _=tray.set_menu(Some(menu));}}}
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run(){
    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent,None))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(|app,_,event|{
            if event.state==ShortcutState::Pressed{let app=app.clone();tauri::async_runtime::spawn(async move{
                let active=app.state::<AppState>().recording.lock().map(|g|g.is_some()).unwrap_or(false);
                let result=if active{stop_impl(app.clone()).await.map(|_|())}else{start_impl(app.clone()).await};
                if let Err(e)=result{log::error!("Shortcut recording action: {e}");}
            });}
        }).build())
        .setup(|app|{
            let data=match std::env::var_os("MEETING_NOTES_DATA_DIR") {Some(value)=>PathBuf::from(value),None=>app.path().app_data_dir()?};
            std::fs::create_dir_all(&data)?;
            app.handle().plugin(tauri_plugin_log::Builder::default().level(log::LevelFilter::Info).targets([
                tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Folder{path:data.join("logs"),file_name:None})
            ]).build())?;
            let db_path=data.join("meetings.db");storage::migrate(&db_path).map_err(std::io::Error::other)?;
            let docs=app.path().document_dir().unwrap_or(data.clone());
            let default_recordings=std::env::var_os("MEETING_NOTES_RECORDINGS_DIR").map(PathBuf::from).unwrap_or_else(||docs.join("Meeting Notes").join("Recordings"));
            std::fs::create_dir_all(&default_recordings)?;
            let ffmpeg=app.path().resource_dir()?.join("bin").join("ffmpeg.exe");
            let ffmpeg_path=if ffmpeg.exists(){ffmpeg}else{PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bin").join("ffmpeg.exe")};
            let settings=storage::load_settings(&db_path).map_err(std::io::Error::other)?;
            for mut m in storage::list(&db_path).map_err(std::io::Error::other)? { if ["TRANSCRIBING","TRANSCRIBED","SUMMARIZING"].contains(&m.status.as_str()){
                m.status=if m.transcript.is_some(){"SUMMARY_FAILED"}else{"TRANSCRIPTION_FAILED"}.into();m.error=Some("Processing was interrupted. Retry to continue.".into());storage::save(&db_path,&m).map_err(std::io::Error::other)?; } }
            app.manage(AppState{db_path,default_recordings,ffmpeg_path,recording:Mutex::new(None),starting:AtomicBool::new(false)});
            tauri::WebviewWindowBuilder::new(app,"main",tauri::WebviewUrl::App("index.html".into()))
                .title("Meeting Notes").inner_size(1260.0,820.0).min_inner_size(860.0,600.0)
                .data_directory(data.join("webview")).build()?;
            if let Err(e)=app.global_shortcut().register(settings.shortcut.as_str()){log::error!("Global shortcut unavailable: {e}");}
            let toggle=MenuItem::with_id(app,"toggle","Start recording",true,None::<&str>)?;let open=MenuItem::with_id(app,"open","Open app",true,None::<&str>)?;
            let sep=PredefinedMenuItem::separator(app)?;let quit=MenuItem::with_id(app,"quit","Quit",true,None::<&str>)?;
            let menu=Menu::with_items(app,&[&toggle,&open,&sep,&quit])?;
            TrayIconBuilder::with_id("main").icon(app.default_window_icon().unwrap().clone()).menu(&menu).on_menu_event(|app,event|match event.id().as_ref(){
                "open"=>{if let Some(window)=app.get_webview_window("main"){let _=window.show();let _=window.set_focus();}},
                "toggle"=>{let app=app.clone();tauri::async_runtime::spawn(async move{let active=app.state::<AppState>().recording.lock().map(|g|g.is_some()).unwrap_or(false);let result=if active{stop_impl(app.clone()).await.map(|_|())}else{start_impl(app.clone()).await};if let Err(e)=result{log::error!("Tray action: {e}");}});},
                "quit"=>{let app=app.clone();tauri::async_runtime::spawn(async move{let active=app.state::<AppState>().recording.lock().map(|g|g.is_some()).unwrap_or(false);
                    if active{let answer=rfd::MessageDialog::new().set_title("Meeting Notes").set_description("A meeting is being recorded. Stop and quit?").set_buttons(rfd::MessageButtons::OkCancel).show();if answer!=rfd::MessageDialogResult::Ok{return;}if let Err(e)=stop_impl(app.clone()).await{log::error!("Stop before quit: {e}");return;}}app.exit(0);});},
                _=>{}
            }).build(app)?;
            Ok(())
        })
        .on_window_event(|window,event|{if let tauri::WindowEvent::CloseRequested{api,..}=event{let state=window.state::<AppState>();let active=state.recording.lock().map(|g|g.is_some()).unwrap_or(false);let minimize=storage::load_settings(&state.db_path).map(|s|s.minimize_to_tray).unwrap_or(true);if active||minimize{api.prevent_close();let _=window.hide();}}})
        .invoke_handler(tauri::generate_handler![get_snapshot,start_recording,stop_recording,rename_meeting,toggle_action,delete_meeting,search_meetings,save_settings,save_api_key,retry_processing,choose_storage,open_storage,open_audio])
        .run(tauri::generate_context!()).expect("Meeting Notes could not start");
}
