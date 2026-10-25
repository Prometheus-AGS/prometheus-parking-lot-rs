//! The Tauri shell: opens the queue file in the app's data directory and exposes four
//! commands to the window. All queue logic lives in `parking-lot-tauri-queue-service`
//! (where the restart test runs). OA-15 adds opt-in isolated fixture recording;
//! the incumbent UI and normal app-data location are unchanged.

use parking_lot_tauri_queue_service::{AppQueue, Collected, Processed, Status};
use tauri::{Manager, State, WebviewWindow};

mod assurance;

use assurance::Assurance;

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Queue `text`; returns the task id.
#[tauri::command]
async fn submit(
    queue: State<'_, AppQueue>,
    assurance: State<'_, Assurance>,
    window: WebviewWindow,
    text: String,
) -> Result<u64, String> {
    let id = queue.submit(&text).await.map_err(message)?;
    assurance
        .record("submit", window.label(), Some(id), text.as_bytes())
        .map_err(message)?;
    Ok(id)
}

/// Run the next waiting task, if there is one.
#[tauri::command]
async fn process_next(
    queue: State<'_, AppQueue>,
    assurance: State<'_, Assurance>,
    window: WebviewWindow,
) -> Result<Option<Processed>, String> {
    let processed = queue.process_next().await.map_err(message)?;
    assurance
        .record(
            "process",
            window.label(),
            processed.as_ref().map(|item| item.task_id),
            &[],
        )
        .map_err(message)?;
    Ok(processed)
}

/// Take the finished results.
#[tauri::command]
async fn collect(
    queue: State<'_, AppQueue>,
    assurance: State<'_, Assurance>,
    window: WebviewWindow,
) -> Result<Vec<Collected>, String> {
    let results = queue.collect().await.map_err(message)?;
    assurance
        .record(
            "collect",
            window.label(),
            None,
            results.len().to_string().as_bytes(),
        )
        .map_err(message)?;
    for item in &results {
        assurance
            .record(
                "result",
                window.label(),
                Some(item.task_id),
                item.text.as_bytes(),
            )
            .map_err(message)?;
    }
    Ok(results)
}

/// Tasks waiting and in progress.
#[tauri::command]
async fn status(queue: State<'_, AppQueue>) -> Result<Status, String> {
    queue.status().await.map_err(message)
}

/// Build and run the application.
///
/// # Panics
///
/// When the queue file cannot be opened or the window system fails to start; there is
/// no useful way to continue.
pub fn run() {
    let assurance = Assurance::from_environment().expect("invalid OA-15 fixture configuration");
    let mut context = tauri::generate_context!();
    if assurance.store_path().is_some() {
        // Fixture windows use nonpersistent webview storage; the only durable
        // application state under test is the explicitly owned redb file.
        for window in &mut context.config_mut().app.windows {
            window.incognito = true;
        }
    }
    tauri::Builder::default()
        .manage(assurance)
        .setup(|app| {
            let path = if let Some(path) = app.state::<Assurance>().store_path() {
                path
            } else {
                // The incumbent per-user app-data location remains the default.
                let dir = app.path().app_data_dir()?;
                std::fs::create_dir_all(&dir)?;
                dir.join("parking-lot.redb")
            };
            let queue = tauri::async_runtime::block_on(AppQueue::open(
                path.to_string_lossy().into_owned(),
            ))?;
            app.manage(queue);
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished) {
                let assurance = webview.state::<Assurance>();
                if let Err(error) = assurance.record(
                    "page-loaded",
                    webview.label(),
                    None,
                    payload.url().as_str().as_bytes(),
                ) {
                    eprintln!("OA15_TRACE_ERROR: {error}");
                    webview.app_handle().exit(1);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            submit,
            process_next,
            collect,
            status
        ])
        .run(context)
        .expect("error while running the Tauri application");
}
