/**
 * A first-run install has no API key, and the AI assistant is the feature the
 * project leads with. These pin the two places that decide whether someone
 * without an Anthropic account finds a way through: the Ollama section of the
 * model menu, and the line the empty chat pane adds about credentials.
 */
import { describe, expect, it } from "vitest";

import {
  credentialHint,
  ollamaMenuMessage,
  providerSupportsTools,
} from "./providers";

describe("ollamaMenuMessage", () => {
  it("says it is still asking while the query is in flight", () => {
    expect(ollamaMenuMessage({ isPending: true, isError: false })).toBe(
      "Looking for local models…",
    );
  });

  /**
   * The distinction that matters: a failed call means Ollama is not there, and
   * the fix is to start it — not to go looking for a model that was never
   * pulled.
   */
  it("blames the daemon, not the model list, when the call fails", () => {
    const message = ollamaMenuMessage({ isPending: false, isError: true });

    expect(message).toContain("isn't running");
    expect(message).not.toContain("pulled");
  });

  it("distinguishes a running Ollama with nothing pulled", () => {
    const message = ollamaMenuMessage({ isPending: false, isError: false });

    expect(message).toContain("running");
    expect(message).toContain("pulled");
  });
});

describe("credentialHint", () => {
  it("names both free routes when Claude has no key", () => {
    const hint = credentialHint("claude", false);

    expect(hint).toContain("Claude needs your own API key");
    expect(hint).toContain("Gemini has a free tier");
    expect(hint).toContain("Ollama runs locally");
  });

  it("does not tell a Gemini user about Gemini's free tier twice", () => {
    const hint = credentialHint("gemini", false) ?? "";

    expect(hint).toContain("free tier");
    expect(hint).toContain("Ollama");
    // "Gemini has a free tier" is the Claude line; the Gemini one says it of
    // the key the user is being asked for, not of some other provider.
    expect(hint).not.toContain("Gemini has a free tier");
  });

  it("says there is nothing to set for a local model", () => {
    expect(credentialHint("ollama", false)).toContain("no key to set");
  });

  /** Nothing to warn about once the key is stored — the hint disappears. */
  it("stays quiet when the key is already stored", () => {
    expect(credentialHint("claude", true)).toBeNull();
    expect(credentialHint("gemini", true)).toBeNull();
  });
});

describe("providerSupportsTools", () => {
  /**
   * Mirrors the gate in `src-tauri/src/ai_commands.rs` —
   * `matches!(conversation.provider.as_str(), "claude" | "ollama" | "gemini")`.
   * The two are checked independently rather than one importing the other
   * (there is no such link across the Rust/TypeScript boundary), so this
   * test is the thing that would catch the day someone extends one side and
   * forgets the other — the toggle buttons would then appear for a provider
   * whose backend still silently streams plain chat, or vice versa.
   */
  it("agrees with the backend's tool-loop gate: claude, ollama and gemini, nothing else", () => {
    expect(providerSupportsTools("claude")).toBe(true);
    expect(providerSupportsTools("ollama")).toBe(true);
    expect(providerSupportsTools("gemini")).toBe(true);
    expect(providerSupportsTools("something-new")).toBe(false);
  });
});
