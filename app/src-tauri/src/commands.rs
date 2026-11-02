//! Commands called by the UI. Heavy work runs on blocking threads.
//! Errors carry a user-facing message; the log (for "Copy log") never
//! contains message content.

use std::path::PathBuf;
use std::sync::Arc;

use cv_core::{ChatSummary, ImportOptions, Importer, InputSource, Secret, probe_all};
use cv_model::{
    Conversation, ConversationData, ConversationId, ConversationKind, Message, Participant,
    SourceReport, Timestamp,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::state::{AppState, OpenArchive};

#[derive(Debug, Serialize)]
pub struct CommandError {
    pub message: String,
}

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(e: E) -> Self {
        Self {
            message: e.to_string(),
        }
    }
}

type CmdResult<T> = Result<T, CommandError>;

fn importers() -> Vec<Box<dyn Importer>> {
    vec![
        Box::new(cv_cva::CvaImporter),
        Box::new(cv_import_whatsapp::WhatsAppImporter),
        #[cfg(feature = "signal")]
        Box::new(cv_import_signal::SignalImporter),
    ]
}

#[derive(Debug, Serialize)]
pub struct Inspection {
    pub importer: Option<String>,
    /// What to ask the user for (e.g. the Signal recovery key).
    pub secret_prompt: Option<String>,
}

#[tauri::command]
pub fn inspect(path: PathBuf) -> Inspection {
    let importers = importers();
    match probe_all(&importers, &InputSource::new(&path)) {
        Some(i) => Inspection {
            importer: Some(i.id().into()),
            secret_prompt: i.secret_prompt().map(str::to_owned),
        },
        None => Inspection {
            importer: None,
            secret_prompt: None,
        },
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct OpenRequest {
    pub path: PathBuf,
    pub secret: Option<String>,
    pub date_order: Option<String>,
    pub self_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatSummaryDto {
    pub id: String,
    pub title: String,
    pub kind: ConversationKind,
    pub participants: usize,
    pub messages: u64,
    pub first: Option<Timestamp>,
    pub last: Option<Timestamp>,
}

impl From<&ChatSummary> for ChatSummaryDto {
    fn from(s: &ChatSummary) -> Self {
        Self {
            id: s.id.to_string(),
            title: s.title.clone(),
            kind: s.kind,
            participants: s.participants,
            messages: s.messages,
            first: s.first,
            last: s.last,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct OpenResult {
    pub handle: u32,
    pub importer: String,
    pub report: SourceReport,
    pub chats: Vec<ChatSummaryDto>,
}

#[tauri::command]
pub async fn open_archive(
    state: State<'_, AppState>,
    request: OpenRequest,
) -> CmdResult<OpenResult> {
    let OpenRequest {
        path,
        secret,
        date_order,
        self_name,
    } = request;
    let opened = tauri::async_runtime::spawn_blocking(move || -> CmdResult<_> {
        let input = InputSource::new(&path);
        let importers = importers();
        let importer = probe_all(&importers, &input)
            .ok_or("This file or folder is not a supported chat archive.")?;
        let mut options = ImportOptions {
            self_name,
            secret: secret.map(Secret::new),
            ..ImportOptions::default()
        };
        if let Some(order) = date_order {
            options.overrides.push(("date_order".into(), order));
        }
        let id = importer.id();
        log::info!("app: opening input with importer {id}");
        let archive = importer.open(&input, &options)?;
        let mut chats: Vec<ChatSummary> = archive
            .conversation_ids()
            .iter()
            .map(|c| archive.summary(c))
            .collect::<Result<_, _>>()?;
        // Most recent chats first, like a messenger.
        chats.sort_by_key(|c| std::cmp::Reverse(c.last));
        Ok(OpenArchive {
            archive,
            importer: id,
            chats,
            loaded: None,
        })
    })
    .await
    .map_err(|e| format!("internal error: {e}"))??;

    let result = OpenResult {
        handle: 0,
        importer: opened.importer.into(),
        report: opened.archive.report().clone(),
        chats: opened.chats.iter().map(ChatSummaryDto::from).collect(),
    };
    let handle = state.insert(opened);
    Ok(OpenResult { handle, ..result })
}

#[derive(Debug, Serialize)]
pub struct ChatInfo {
    pub conversation: Conversation,
    pub participants: Vec<Participant>,
    pub message_count: usize,
}

fn loaded_chat(state: &AppState, handle: u32, chat: &str) -> CmdResult<Arc<ConversationData>> {
    let archives = state.archives.lock().expect("state lock");
    let open = archives
        .get(&handle)
        .ok_or("The archive is no longer open.")?;
    match &open.loaded {
        Some((id, data)) if id.as_str() == chat => Ok(data.clone()),
        _ => Err("This chat is not loaded.".into()),
    }
}

#[tauri::command]
pub async fn open_chat(
    state: State<'_, AppState>,
    app: AppHandle,
    handle: u32,
    chat: String,
) -> CmdResult<ChatInfo> {
    let id = ConversationId::new(chat);
    // Decoding can take a while; do it without holding the state lock.
    let data = tauri::async_runtime::spawn_blocking(move || -> CmdResult<ConversationData> {
        let state = app.state::<AppState>();
        let archives = state.archives.lock().expect("state lock");
        let open = archives
            .get(&handle)
            .ok_or("The archive is no longer open.")?;
        Ok(open.archive.load(&id)?)
    })
    .await
    .map_err(|e| format!("internal error: {e}"))??;

    let info = ChatInfo {
        conversation: data.conversation.clone(),
        participants: data.participants.clone(),
        message_count: data.messages.len(),
    };
    let mut archives = state.archives.lock().expect("state lock");
    let open = archives
        .get_mut(&handle)
        .ok_or("The archive is no longer open.")?;
    open.loaded = Some((data.conversation.id.clone(), Arc::new(data)));
    Ok(info)
}

/// A page of messages. Raw source records stay in the backend; they are
/// large and only needed for exports.
#[tauri::command]
pub fn messages(
    state: State<'_, AppState>,
    handle: u32,
    chat: String,
    offset: usize,
    limit: usize,
) -> CmdResult<Vec<Message>> {
    let data = loaded_chat(&state, handle, &chat)?;
    let end = (offset + limit.min(2000)).min(data.messages.len());
    Ok(data.messages[offset.min(end)..end]
        .iter()
        .map(|m| {
            let mut m = m.clone();
            m.extra.raw = None;
            m
        })
        .collect())
}

/// Position of a message, e.g. to jump to a quoted message.
#[tauri::command]
pub fn message_index(
    state: State<'_, AppState>,
    handle: u32,
    chat: String,
    message: String,
) -> CmdResult<Option<usize>> {
    let data = loaded_chat(&state, handle, &chat)?;
    Ok(data.messages.iter().position(|m| m.id.as_str() == message))
}

#[derive(Debug, Serialize)]
pub struct ExportResult {
    pub messages: u64,
    pub media_written: u64,
    pub media_failed: u64,
    pub lossless: bool,
}

#[tauri::command]
pub async fn export_chat(
    app: AppHandle,
    handle: u32,
    chat: String,
    format: String,
    path: PathBuf,
) -> CmdResult<ExportResult> {
    tauri::async_runtime::spawn_blocking(move || -> CmdResult<ExportResult> {
        let state = app.state::<AppState>();
        let archives = state.archives.lock().expect("state lock");
        let open = archives
            .get(&handle)
            .ok_or("The archive is no longer open.")?;
        let id = ConversationId::new(chat);
        // The save dialog has already asked before replacing a file.
        let file = std::fs::File::create(&path)?;
        if format == "cva" {
            let summary = cv_cva::write(&*open.archive, &id, file)?;
            return Ok(ExportResult {
                messages: summary.messages,
                media_written: summary.media_written,
                media_failed: summary.media_failed,
                lossless: true,
            });
        }
        let exporter = cv_export::by_name(&format).ok_or("Unknown export format.")?;
        let data = open.archive.load(&id)?;
        let mut out = std::io::BufWriter::new(file);
        exporter.export(&data, open.archive.report(), &mut out)?;
        std::io::Write::flush(&mut out)?;
        Ok(ExportResult {
            messages: data.messages.len() as u64,
            media_written: 0,
            media_failed: 0,
            lossless: exporter.lossless(),
        })
    })
    .await
    .map_err(|e| format!("internal error: {e}"))?
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportProgress {
    pub done: usize,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct ExportedFile {
    pub chat: String,
    pub file: String,
    pub messages: u64,
    pub media_written: u64,
    pub media_failed: u64,
}

#[derive(Debug, Serialize)]
pub struct ExportFailure {
    pub chat: String,
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct BatchExportResult {
    pub folder: String,
    pub exported: Vec<ExportedFile>,
    pub failed: Vec<ExportFailure>,
}

/// Exports each chat to its own file in `folder` (one decrypted ZIP per
/// chat for `cva`). Never overwrites: taken names get " (2)" etc.
/// Emits `export-progress` events.
#[tauri::command]
pub async fn export_chats(
    app: AppHandle,
    handle: u32,
    chats: Vec<String>,
    format: String,
    folder: PathBuf,
) -> CmdResult<BatchExportResult> {
    use tauri::Emitter;

    /// Chats decoded per pass over the backup (bounds memory).
    const BATCH: usize = 50;

    tauri::async_runtime::spawn_blocking(move || -> CmdResult<BatchExportResult> {
        let exporter = match format.as_str() {
            "cva" => None,
            f => Some(cv_export::by_name(f).ok_or("Unknown export format.")?),
        };
        let ext = exporter.as_ref().map_or("zip", |e| e.extension());
        if !folder.is_dir() {
            return Err("The export folder does not exist.".into());
        }
        let state = app.state::<AppState>();
        let archives = state.archives.lock().expect("state lock");
        let open = archives
            .get(&handle)
            .ok_or("The archive is no longer open.")?;
        let ids: Vec<ConversationId> = chats.into_iter().map(ConversationId::new).collect();
        let total = ids.len();
        let mut result = BatchExportResult {
            folder: folder.display().to_string(),
            exported: Vec::new(),
            failed: Vec::new(),
        };
        let _ = app.emit("export-progress", ExportProgress { done: 0, total });
        log::info!("app: batch export of {total} chats");

        for batch in ids.chunks(BATCH) {
            let loaded = match open.archive.load_many(batch) {
                Ok(loaded) => loaded,
                Err(e) => {
                    result.failed.extend(batch.iter().map(|id| ExportFailure {
                        chat: id.to_string(),
                        error: e.to_string(),
                    }));
                    continue;
                }
            };
            for data in loaded {
                let chat = data.conversation.id.to_string();
                let stem = cv_core::safe_file_stem(&data.conversation.title);
                let path = cv_core::unique_path(&folder, &stem, ext);
                let outcome = (|| -> CmdResult<ExportedFile> {
                    let file = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)?;
                    let messages = data.messages.len() as u64;
                    let (media_written, media_failed) = match &exporter {
                        None => {
                            let s = cv_cva::write_data(&*open.archive, data, file)?;
                            (s.media_written, s.media_failed)
                        }
                        Some(exporter) => {
                            let mut out = std::io::BufWriter::new(file);
                            exporter.export(&data, open.archive.report(), &mut out)?;
                            std::io::Write::flush(&mut out)?;
                            (0, 0)
                        }
                    };
                    Ok(ExportedFile {
                        chat: chat.clone(),
                        file: path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        messages,
                        media_written,
                        media_failed,
                    })
                })();
                match outcome {
                    Ok(file) => result.exported.push(file),
                    Err(e) => {
                        log::warn!("app: a chat could not be exported");
                        let _ = std::fs::remove_file(&path);
                        result.failed.push(ExportFailure {
                            chat,
                            error: e.message,
                        });
                    }
                }
                let done = result.exported.len() + result.failed.len();
                let _ = app.emit("export-progress", ExportProgress { done, total });
            }
        }
        log::info!(
            "app: batch export finished: {} written, {} failed",
            result.exported.len(),
            result.failed.len()
        );
        Ok(result)
    })
    .await
    .map_err(|e| format!("internal error: {e}"))?
}

#[tauri::command]
pub fn close_archive(state: State<'_, AppState>, handle: u32) {
    state.archives.lock().expect("state lock").remove(&handle);
    state
        .blobs
        .lock()
        .expect("blob lock")
        .clear_prefix(&format!("{handle}/"));
    log::info!("app: archive closed");
}

/// The in-memory log for "Copy log" (redacted, ADR 0003).
#[tauri::command]
pub fn log_snapshot() -> Vec<String> {
    cv_core::logging::snapshot()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// `auto`, `light` or `dark`.
    pub theme: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "auto".into(),
        }
    }
}

/// Settings are the only thing persisted (ADR 0003): no chat data.
fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("settings.json"))
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    settings_path(&app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, settings: Settings) -> CmdResult<()> {
    if !matches!(settings.theme.as_str(), "auto" | "light" | "dark") {
        return Err("invalid theme".into());
    }
    let path = settings_path(&app).ok_or("no settings folder")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(&settings)?)?;
    Ok(())
}
