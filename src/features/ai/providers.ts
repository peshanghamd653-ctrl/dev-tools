/**
 * What the AI page says about credentials before anyone has any.
 *
 * A fresh install has no key, and the two ways around that — Gemini's free
 * tier and Ollama's local models — are not visible from the chat pane. The
 * strings live here rather than inline so both of them can be pinned by a
 * test: this is the copy that decides whether someone without an Anthropic
 * account concludes the flagship feature is unusable.
 */
import { PROVIDER_LABELS } from "./store";

/** Shape of the Ollama model query, narrowed to what the message depends on. */
export interface OllamaQueryState {
  isPending: boolean;
  isError: boolean;
}

/**
 * The Ollama section of the model menu when it has no models to list. Three
 * different nothings, and telling them apart is the point: "not running" is
 * fixed by starting Ollama, "no models" by pulling one.
 */
export function ollamaMenuMessage(query: OllamaQueryState): string {
  if (query.isPending) return "Looking for local models…";
  if (query.isError) {
    return "Ollama isn't running — start it, then reopen this menu";
  }
  return "Ollama is running, but no models are pulled yet";
}

/**
 * Providers whose conversations can drive the agentic tool loop —
 * `devos_ai::run_agent` / `run_agent_ollama` in the Rust layer.
 *
 * Gemini is deliberately absent: `crates/devos-ai/src/providers/gemini.rs`
 * documents why (Gemini has function calling, but nothing here has adapted
 * its streaming shape to it yet). Checked in one place so the toggle buttons
 * below and the backend's own gate in `ai_commands.rs` cannot drift apart —
 * they already agreed on "claude" alone once; the day this file forgets to
 * add a name the backend just enabled is the day tools become a toggle that
 * appears in the UI but silently no-ops.
 */
const TOOL_CAPABLE_PROVIDERS = new Set(["claude", "ollama"]);

export function providerSupportsTools(provider: string): boolean {
  return TOOL_CAPABLE_PROVIDERS.has(provider);
}

/**
 * The one line the empty chat pane adds about credentials, or `null` when
 * there is nothing worth saying because the key is already stored.
 */
export function credentialHint(
  provider: string,
  hasKey: boolean,
): string | null {
  if (provider === "ollama") {
    return "Ollama runs on this machine, so there is no key to set.";
  }
  if (hasKey) return null;
  if (provider === "gemini") {
    return "Gemini needs your own API key, and its free tier does not require a billing account. Ollama runs locally with no key at all.";
  }
  const label = PROVIDER_LABELS[provider] ?? provider;
  return `${label} needs your own API key. Gemini has a free tier and Ollama runs locally with no key — both are in the model menu beside New chat.`;
}
