use crate::services::tts_service::call_openai_tts;
use crate::state::AppState;
use axum::{
    extract::{Json, State},
    http::StatusCode,
};
use futures::future::join_all;
use serde::Deserialize;
use serde_json::json;
use std::{env, fs};
use tokio::task;

// For chunking Unicode text
use crate::utils::chunk_text_unicode::chunk_text_unicode;
use crate::utils::concat_mp3::concat_mp3;

// Add chrono for date/time folder naming
use chrono::Local;

#[derive(Deserialize)]
pub struct UserInput {
    pub input: String,
}

pub async fn speech(
    State(_state): State<AppState>,
    Json(payload): Json<UserInput>,
) -> (StatusCode, Json<serde_json::Value>) {
    println!(
        "Speech endpoint called with input length: {}",
        payload.input.len()
    );

    // 1) Check OPENAI_API_KEY
    let api_key = match env::var("OPENAI_API_KEY") {
        Ok(k) => {
            println!("Found OPENAI_API_KEY in environment");
            k
        }
        Err(_) => {
            println!("Error: Missing OPENAI_API_KEY environment variable");
            let err = json!({ "error": "Missing OPENAI_API_KEY" });
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(err));
        }
    };

    // 2) Chunk text at Unicode boundaries
    println!("Calling chunk_text_unicode...");
    let chunks = chunk_text_unicode(&payload.input, 4096);
    println!("Finished chunking; got {} chunk(s)", chunks.len());

    if chunks.is_empty() {
        let err = json!({ "error": "No text provided." });
        println!("No text => returning 400");
        return (StatusCode::BAD_REQUEST, Json(err));
    }

    // 3) Create a folder named with the current date/time, e.g. "2025-03-21-12:25"
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d-%H:%M").to_string();
    let folder_name = timestamp.clone();
    if let Err(e) = fs::create_dir_all(&folder_name) {
        let err = json!({ "error": format!("Failed to create directory {folder_name}: {e}") });
        println!("Error creating directory {}: {}", folder_name, e);
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(err));
    }

    println!("Created or verified existence of folder: {}", folder_name);

    // 4) For each chunk, spawn a parallel TTS task
    println!("Spawning parallel tasks for TTS calls...");
    let mut tasks = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let api_key_cloned = api_key.clone();
        let chunk_cloned = chunk.clone();
        let voice = "onyx".to_string();
        let index = i + 1;
        println!("  -> Chunk #{index}: length = {} graphemes", chunk.len());

        // Construct the output path for this chunk
        let chunk_filename = format!("{}/speech-chunk-{}.mp3", folder_name, index);

        tasks.push(task::spawn(async move {
            println!("  -> [Task {index}] calling TTS...");
            let tts_result = call_openai_tts(&api_key_cloned, &chunk_cloned, &voice).await;
            match tts_result {
                Ok(bytes) => match fs::write(&chunk_filename, &bytes) {
                    Ok(_) => {
                        println!("  -> [Task {index}] wrote {chunk_filename}");
                        Ok(chunk_filename)
                    }
                    Err(e) => {
                        let msg = format!("Failed to write {chunk_filename}: {e}");
                        println!("  -> [Task {index}] error: {msg}");
                        Err(msg)
                    }
                },
                Err(msg) => {
                    let full_msg = format!("Chunk {index} TTS error: {msg}");
                    println!("  -> [Task {index}] TTS error: {full_msg}");
                    Err(full_msg)
                }
            }
        }));
    }

    // 5) Wait for all tasks
    println!("All tasks spawned; waiting on join_all...");
    let results = join_all(tasks).await;
    println!("join_all completed; analyzing results...");

    // Accumulate the chunk filenames
    let mut saved_files = Vec::new();

    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(Ok(filename)) => {
                println!("Task #{} succeeded => {}", i + 1, filename);
                saved_files.push(filename);
            }
            Ok(Err(e)) => {
                println!("Task #{} returned an error => {}", i + 1, e);
                let err = json!({ "error": format!("Task #{} error: {e}", i+1) });
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(err));
            }
            Err(join_err) => {
                println!("Task #{} panicked or cancelled => {}", i + 1, join_err);
                let err = json!({ "error": format!("Join error on task #{}: {join_err}", i+1) });
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(err));
            }
        }
    }

    // 6) Now we do a naive merge of those chunk MP3s
    let final_mp3_path = format!("{}/{}.mp3", folder_name, timestamp);
    println!(
        "Merging {} chunk(s) => {}",
        saved_files.len(),
        final_mp3_path
    );
    let saved_files_ref: Vec<&str> = saved_files.iter().map(|s| s.as_str()).collect();
    if let Err(e) = concat_mp3(&saved_files_ref, &final_mp3_path) {
        println!("Error merging MP3: {}", e);
        let err = json!({ "error": format!("Failed to merge mp3: {e}") });
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(err));
    }

    // 7) Return success
    println!("All chunks processed + merged => {}", final_mp3_path);
    let response = json!({
        "message": "All chunks processed in parallel and merged",
        "files": saved_files,
        "merged_file": final_mp3_path,
    });
    (StatusCode::OK, Json(response))
}
