//! AI + secrets IPC commands.
//!
//! Redaction rule: secret VALUES flow only from the UI into the store
//! (`secret_set`) and from the store into providers. No command returns a
//! secret value to the webview.

use devos_ai::{repo as ai_repo, AiDelta, ChatMessage, ChatTurn, Conversation, StreamRequest};
use devos_kernel::repo;
use tauri::ipc::Channel;
use tauri::State;
use tokio::sync::mpsc;

use crate::state::AppState;

// ---- Secrets (redacted surface) ----

#[tauri::command]
pub async fn secret_set(
    state: State<'_, AppState>,
    name: String,
    value: String,
) -> Result<(), String> {
    state
        .secrets
        .set(&name, &value)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn secret_list(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(state
        .secrets
        .list()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|meta| meta.name)
        .collect())
}

#[tauri::command]
pub async fn secret_delete(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state.secrets.delete(&name).await.map_err(|e| e.to_string())
}

// ---- Conversations ----

#[tauri::command]
pub async fn ai_conversations_list(
    state: State<'_, AppState>,
) -> Result<Vec<Conversation>, String> {
    ai_repo::list_conversations(&state.kernel.pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_conversation_create(
    state: State<'_, AppState>,
    provider: String,
    model: String,
) -> Result<Conversation, String> {
    ai_repo::create_conversation(&state.kernel.pool, &provider, &model)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_conversation_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    ai_repo::delete_conversation(&state.kernel.pool, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_messages(
    state: State<'_, AppState>,
    conversation_id: String,
) -> Result<Vec<ChatMessage>, String> {
    ai_repo::messages(&state.kernel.pool, &conversation_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_ollama_models(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let base = repo::get_setting(&state.kernel.pool, "ai.ollama.url")
        .await
        .map_err(|e| e.to_string())?;
    state
        .ai
        .ollama
        .list_models(base.as_deref())
        .await
        .map_err(|e| e.to_string())
}

/// Generate a commit message from the staged diff. One-shot, no streaming.
#[tauri::command]
pub async fn ai_commit_message(
    state: State<'_, AppState>,
    path: String,
    provider: String,
    model: String,
) -> Result<String, String> {
    let repo_path = std::path::PathBuf::from(&path);
    let diff = devos_git::staged_diff(&repo_path, 24 * 1024)
        .await
        .map_err(|e| e.to_string())?;
    if diff.trim().is_empty() {
        return Err("nothing is staged".into());
    }

    let api_key = state
        .secrets
        .get("anthropic-api-key")
        .await
        .map_err(|e| e.to_string())?;
    let base_url = repo::get_setting(&state.kernel.pool, "ai.ollama.url")
        .await
        .map_err(|e| e.to_string())?;

    let system = "You write git commit messages. Reply with ONLY the commit message: \
        an imperative subject line under 72 characters, optionally followed by a blank \
        line and a short body. Use conventional commit prefixes (feat:, fix:, refactor:, \
        docs:, chore:, test:) when they fit. No markdown fences, no quotes, no explanations.";
    let turns = vec![ChatTurn {
        role: "user".into(),
        content: format!("Write a commit message for these staged changes:\n\n{diff}"),
    }];

    let text = state
        .ai
        .complete_once(
            &provider,
            StreamRequest {
                model: &model,
                system: Some(system),
                messages: &turns,
                api_key: api_key.as_deref(),
                base_url: base_url.as_deref(),
            },
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(text.trim().trim_matches('`').trim().to_string())
}

/// Build a compact system prompt describing the attached project.
async fn project_context(project_path: &str) -> String {
    let repo_path = std::path::Path::new(project_path);
    let mut ctx = format!(
        "You are the DevOS AI assistant inside the user's development environment. \
         The active project is at {project_path}."
    );
    if let Ok((info, entries)) = devos_git::status(repo_path).await {
        if info.is_repo {
            ctx.push_str(&format!(
                "\nGit: branch {}, {} changed file(s).",
                info.branch.as_deref().unwrap_or("detached"),
                entries.len()
            ));
            if !entries.is_empty() {
                let names: Vec<&str> = entries.iter().take(20).map(|e| e.path.as_str()).collect();
                ctx.push_str(&format!(" Changed: {}.", names.join(", ")));
            }
        }
    }
    ctx.push_str("\nBe concise and practical.");
    ctx
}

/// Send a user message and stream the assistant reply over `on_delta`.
/// Resolves when the reply is complete (already persisted).
#[tauri::command]
pub async fn ai_send(
    state: State<'_, AppState>,
    conversation_id: String,
    content: String,
    project_path: Option<String>,
    on_delta: Channel<AiDelta>,
) -> Result<ChatMessage, String> {
    let pool = &state.kernel.pool;
    let conversation = ai_repo::get_conversation(pool, &conversation_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("conversation {conversation_id} not found"))?;

    ai_repo::append_message(pool, &conversation_id, "user", &content)
        .await
        .map_err(|e| e.to_string())?;

    let history = ai_repo::messages(pool, &conversation_id)
        .await
        .map_err(|e| e.to_string())?;
    let turns: Vec<ChatTurn> = history
        .iter()
        .map(|m| ChatTurn {
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect();

    let api_key = state
        .secrets
        .get("anthropic-api-key")
        .await
        .map_err(|e| e.to_string())?;
    let base_url = repo::get_setting(pool, "ai.ollama.url")
        .await
        .map_err(|e| e.to_string())?;

    let provider = state
        .ai
        .provider(&conversation.provider)
        .map_err(|e| e.to_string())?;

    let system = match &project_path {
        Some(path) => Some(project_context(path).await),
        None => None,
    };

    // Forward provider frames to the webview channel as they arrive.
    let (tx, mut rx) = mpsc::unbounded_channel::<AiDelta>();
    let forward_channel = on_delta.clone();
    let forwarder = tauri::async_runtime::spawn(async move {
        while let Some(delta) = rx.recv().await {
            if forward_channel.send(delta).is_err() {
                break;
            }
        }
    });

    let result = provider
        .stream_chat(
            StreamRequest {
                model: &conversation.model,
                system: system.as_deref(),
                messages: &turns,
                api_key: api_key.as_deref(),
                base_url: base_url.as_deref(),
            },
            &tx,
        )
        .await;
    drop(tx);
    let _ = forwarder.await;

    match result {
        Ok(full_text) => {
            let message = ai_repo::append_message(pool, &conversation_id, "assistant", &full_text)
                .await
                .map_err(|e| e.to_string())?;
            let _ = on_delta.send(AiDelta::Done);
            Ok(message)
        }
        Err(e) => {
            let message = e.to_string();
            let _ = on_delta.send(AiDelta::Error {
                message: message.clone(),
            });
            Err(message)
        }
    }
}
