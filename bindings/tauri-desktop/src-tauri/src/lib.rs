//! The Tauri shell: opens the queue file in the app's data directory and exposes four
//! commands to the window. All queue logic lives in `parking-lot-tauri-queue-service`
//! (where the restart test runs); these functions only translate errors to strings.

use parking_lot_tauri_queue_service::{AppQueue, Collected, Processed, Status};
use tauri::{Manager, State};

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

/// Queue `text`; returns the task id.
#[tauri::command]
async fn submit(queue: State<'_, AppQueue>, text: String) -> Result<u64, String> {
    queue.submit(&text).await.map_err(message)
}

/// Run the next waiting task, if there is one.
#[tauri::command]
async fn process_next(queue: State<'_, AppQueue>) -> Result<Option<Processed>, String> {
    queue.process_next().await.map_err(message)
}

/// Take the finished results.
#[tauri::command]
async fn collect(queue: State<'_, AppQueue>) -> Result<Vec<Collected>, String> {
    queue.collect().await.map_err(message)
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
    tauri::Builder::default()
        .setup(|app| {
            // The OS's per-user, per-app data directory: it survives updates and is
            // where an app's durable state belongs.
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let path = dir.join("parking-lot.redb");
            let queue = tauri::async_runtime::block_on(AppQueue::open(
                path.to_string_lossy().into_owned(),
            ))?;
            app.manage(queue);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            submit,
            process_next,
            collect,
            status
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}
