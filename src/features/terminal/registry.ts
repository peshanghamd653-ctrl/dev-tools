/**
 * Module-level registry of xterm instances. Instances (and their DOM
 * containers, including scrollback) survive React unmounts, so navigating
 * away from the terminal page never loses a shell.
 */
import { FitAddon } from "@xterm/addon-fit";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";

export interface TermInstance {
  term: Terminal;
  fit: FitAddon;
  container: HTMLDivElement;
  opened: boolean;
}

const instances = new Map<string, TermInstance>();

export function createInstance(id: string): TermInstance {
  const term = new Terminal({
    fontFamily: '"JetBrains Mono Variable", ui-monospace, monospace',
    fontSize: 13,
    lineHeight: 1.25,
    cursorBlink: true,
    scrollback: 5000,
    theme: {
      background: "#121214",
      foreground: "#d7d7de",
      cursor: "#8f8ff5",
      cursorAccent: "#121214",
      selectionBackground: "rgba(120, 119, 240, 0.32)",
      black: "#1f1f23",
      brightBlack: "#5c5c66",
    },
  });
  const fit = new FitAddon();
  term.loadAddon(fit);

  const container = document.createElement("div");
  container.style.width = "100%";
  container.style.height = "100%";

  const instance: TermInstance = { term, fit, container, opened: false };
  instances.set(id, instance);
  return instance;
}

export function getInstance(id: string): TermInstance | undefined {
  return instances.get(id);
}

export function disposeInstance(id: string) {
  const instance = instances.get(id);
  if (!instance) return;
  instance.term.dispose();
  instance.container.remove();
  instances.delete(id);
}
