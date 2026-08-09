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

/// Which stored secret a provider authenticates with.
///
/// Ollama is local and needs none. Returning `None` rather than an empty
/// string keeps "no key configured" distinguishable from "key is blank",
/// which is what lets the setup card appear instead of a 401.
fn key_secret_for(provider: &str) -> Option<&'static str> {
    match provider {
        "claude" => Some("anthropic-api-key"),
        "gemini" => Some("gemini-api-key"),
        _ => None,
    }
}

/// Whether a provider's endpoint is user-configurable, and under which setting.
///
/// **Only Ollama's is**, and that asymmetry is the whole point of this function
/// existing rather than one shared variable.
///
/// `ai.ollama.url` is a legitimate setting: it points DevOS at an Ollama server
/// that may be on another machine. But it was being read once and passed to
/// *whichever* provider the conversation used. Claude was unaffected by luck —
/// it ignores the field and posts to a hardcoded constant. Gemini reads it,
/// builds `{base}/v1beta/models/...` and attaches `x-goog-api-key`.
///
/// So configuring a remote Ollama endpoint sent the user's Gemini key, the
/// system prompt and the whole conversation to that host. No attacker was
/// required — that is simply what happened to anyone who used both providers
/// and pointed Ollama somewhere. An attacker who could reach `settings_set`
/// got the same outcome durably, against a setting no screen displays.
///
/// Returning the setting name per provider means a provider that gains a
/// configurable endpoint later has to say so here, rather than silently
/// inheriting one belonging to something else.
fn base_url_setting_for(provider: &str) -> Option<&'static str> {
    match provider {
        "ollama" => Some("ai.ollama.url"),
        // Claude and Gemini talk to their vendor and nowhere else. A key is
        // only safe to send to an endpoint the user cannot redirect.
        _ => None,
    }
}

/// The endpoint override for a provider, if that provider has a configurable
/// one and it is set.
async fn provider_base_url(state: &AppState, provider: &str) -> Result<Option<String>, String> {
    let Some(setting) = base_url_setting_for(provider) else {
        return Ok(None);
    };
    repo::get_setting(&state.kernel.pool, setting)
        .await
        .map_err(|e| e.to_string())
}

/// The API key for a provider, if that provider needs one and it is stored.
async fn provider_key(state: &AppState, provider: &str) -> Result<Option<String>, String> {
    let Some(name) = key_secret_for(provider) else {
        return Ok(None);
    };
    Ok(state
        .secrets
        .get(name)
        .await
        .map_err(|e| e.to_string())?
        .filter(|value| !value.trim().is_empty()))
}

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
        .map_err(|e| e.to_string())?;
    // The **name**, never the value — and enforced by the shape of
    // `AuditEvent::SecretSet`, which has no field a value could travel in.
    // The audit log is plaintext in the same database the encryption exists
    // to survive, so it is the last place a value may ever appear. Recorded
    // after the write succeeds: a refused `set` changed nothing.
    state
        .kernel
        .audit(devos_kernel::audit::AuditEvent::SecretSet { name })
        .await;
    Ok(())
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
    state
        .secrets
        .delete(&name)
        .await
        .map_err(|e| e.to_string())?;
    // Deletion is the half a `secret.set` row cannot imply. "The token stopped
    // working on the 3rd" is answerable only if the removal left a mark.
    state
        .kernel
        .audit(devos_kernel::audit::AuditEvent::SecretDeleted { name })
        .await;
    Ok(())
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

    let api_key = provider_key(&state, &provider).await?;
    let base_url = provider_base_url(&state, &provider).await?;

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

/// Build a compact system prompt describing the attached project, including
/// saved long-term memory (all entries are user-visible in the Memory panel).
async fn project_context(pool: &sqlx::SqlitePool, project_path: &str) -> String {
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
    let project = devos_index::project_key(project_path);
    if let Ok(memories) = devos_ai::repo::memory_list(pool, &project).await {
        if !memories.is_empty() {
            ctx.push_str("\nSaved project memory (facts the user asked to remember):");
            for entry in memories.iter().take(50) {
                ctx.push_str(&format!("\n- {}", entry.content));
            }
        }
    }
    ctx.push_str("\nBe concise and practical.");
    ctx
}

// ---- Long-term memory (visible/editable surface for the Memory panel) ----

#[tauri::command]
pub async fn ai_memory_list(
    state: State<'_, AppState>,
    project_path: String,
) -> Result<Vec<devos_ai::MemoryEntry>, String> {
    ai_repo::memory_list(&state.kernel.pool, &devos_index::project_key(&project_path))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_memory_add(
    state: State<'_, AppState>,
    project_path: String,
    content: String,
) -> Result<devos_ai::MemoryEntry, String> {
    ai_repo::memory_add(
        &state.kernel.pool,
        &devos_index::project_key(&project_path),
        &content,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ai_memory_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    ai_repo::memory_delete(&state.kernel.pool, &id)
        .await
        .map_err(|e| e.to_string())
}

/// Resolve a pending per-call tool approval (see approvals.rs).
#[tauri::command]
pub async fn ai_tool_respond(
    state: State<'_, AppState>,
    id: String,
    approved: bool,
) -> Result<bool, String> {
    Ok(state.approvals.resolve(&id, approved))
}

/// Send a user message and stream the assistant reply over `on_delta`.
/// Resolves when the reply is complete (already persisted).
#[tauri::command]
pub async fn ai_send(
    state: State<'_, AppState>,
    conversation_id: String,
    content: String,
    project_path: Option<String>,
    tools_enabled: Option<bool>,
    write_tools_enabled: Option<bool>,
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

    let api_key = provider_key(&state, &conversation.provider).await?;
    let base_url = provider_base_url(&state, &conversation.provider).await?;

    let provider = state
        .ai
        .provider(&conversation.provider)
        .map_err(|e| e.to_string())?;

    let system = match &project_path {
        Some(path) => Some(project_context(pool, path).await),
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

    // The tools grants: read-only tools exist only when the user enabled
    // tools for this send AND a project is attached; write/execute tools
    // additionally need the second grant, and each call still goes through
    // per-call approval (ADR-0005).
    //
    // Claude and Ollama drive the agentic loop (`devos_ai::run_agent` /
    // `run_agent_ollama`); Gemini does not yet — see the note at the top of
    // `crates/devos-ai/src/providers/gemini.rs` for why, and
    // `providers::mod`'s doc comment for how the two implementations differ.
    let use_tools = tools_enabled.unwrap_or(false)
        && project_path.is_some()
        && matches!(conversation.provider.as_str(), "claude" | "ollama");
    let use_write_tools = use_tools && write_tools_enabled.unwrap_or(false);

    let result = if use_tools {
        let root = std::path::PathBuf::from(project_path.clone().unwrap_or_default());
        let mut defs = crate::tools::tool_defs();
        // The gate exists at *both* tiers. The read tier needs one because
        // `save_memory` is mutating — it writes text that lands in the system
        // prompt of every later conversation — and per-call approval is the
        // only thing that puts that text in front of the user first (SEC-002).
        let gate = std::sync::Arc::new(crate::approvals::ChannelApprovalGate::new(
            state.approvals.clone(),
            on_delta.clone(),
        ));
        let executor = if use_write_tools {
            defs.extend(crate::tools::write_tool_defs());
            crate::tools::ProjectTools::with_write_access(root, state.kernel.pool.clone(), gate)
        } else {
            crate::tools::ProjectTools::with_approval(root, state.kernel.pool.clone(), gate)
        };
        match conversation.provider.as_str() {
            "claude" => {
                let key = api_key
                    .as_deref()
                    .ok_or("no Anthropic API key configured")?;
                devos_ai::run_agent(
                    &state.ai.claude,
                    key,
                    &conversation.model,
                    system.as_deref(),
                    &turns,
                    &defs,
                    &executor,
                    &tx,
                )
                .await
            }
            "ollama" => {
                devos_ai::run_agent_ollama(
                    &state.ai.ollama,
                    base_url.as_deref(),
                    &conversation.model,
                    system.as_deref(),
                    &turns,
                    &defs,
                    &executor,
                    &tx,
                )
                .await
            }
            // `use_tools` above is the only place this branch is reachable
            // from, and it already restricts the provider to these two.
            _ => unreachable!("use_tools only allows claude or ollama"),
        }
    } else {
        provider
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
            .await
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A key may only be sent to an endpoint the user cannot redirect.
    ///
    /// This pins SEC-101. `ai.ollama.url` was read once and handed to whichever
    /// provider was active, so pointing DevOS at a remote Ollama server also
    /// sent the Gemini API key, the system prompt and the whole conversation to
    /// that host. Claude escaped only because it ignores the field.
    ///
    /// The pairing below is the invariant: a provider that authenticates with a
    /// stored key must have no user-configurable endpoint. If a future provider
    /// needs both, that is a deliberate decision and this test is where it has
    /// to be argued.
    #[test]
    fn no_provider_both_carries_a_key_and_accepts_a_redirectable_endpoint() {
        for provider in ["claude", "gemini", "ollama", "unknown"] {
            let key = key_secret_for(provider);
            let base = base_url_setting_for(provider);
            assert!(
                key.is_none() || base.is_none(),
                "provider {provider:?} sends secret {key:?} to an endpoint \
                 configurable through setting {base:?} — a user or an attacker \
                 who sets that value receives the key"
            );
        }
    }

    #[test]
    fn only_ollama_has_a_configurable_endpoint() {
        assert_eq!(base_url_setting_for("ollama"), Some("ai.ollama.url"));
        assert_eq!(base_url_setting_for("claude"), None);
        assert_eq!(base_url_setting_for("gemini"), None);
        // Unknown providers get no endpoint rather than inheriting one.
        assert_eq!(base_url_setting_for("something-new"), None);
    }
}
