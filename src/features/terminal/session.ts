import { Channel } from "@tauri-apps/api/core";

import {
  inDesktopShell,
  ipc,
  type TermEvent,
} from "@/shared/ipc/client";
import { createInstance, disposeInstance, getInstance } from "./registry";
import { useTerminalStore } from "./store";

/** Spawn a backend pty, wire its stream into a fresh xterm instance. */
export async function createTerminalSession(cwd?: string): Promise<void> {
  if (!inDesktopShell) {
    throw new Error("Terminals require the desktop shell");
  }

  const channel = new Channel<TermEvent>();
  // Buffer frames that arrive before the xterm instance exists (the shell
  // banner can beat us to it).
  const pending: TermEvent[] = [];
  let deliver: (event: TermEvent) => void = (event) => {
    pending.push(event);
  };
  channel.onmessage = (event) => deliver(event);

  const info = await ipc.termCreate({ cwd, cols: 80, rows: 24 }, channel);
  const instance = createInstance(info.id);

  const apply = (event: TermEvent) => {
    if (event.kind === "output") {
      instance.term.write(new Uint8Array(event.data.bytes));
    } else {
      useTerminalStore.getState().setExited(info.id);
      const suffix =
        event.data.code != null ? ` with code ${event.data.code}` : "";
      instance.term.write(
        `\r\n\x1b[90m[process exited${suffix}]\x1b[0m\r\n`,
      );
    }
  };
  pending.forEach(apply);
  deliver = apply;

  instance.term.onData((data) => {
    void ipc.termWrite(info.id, data).catch(() => {
      /* session already gone */
    });
  });

  useTerminalStore.getState().addLive(info);
}

/** Kill the backend session (if any) and drop the local instance. */
export async function closeTerminalSession(id: string): Promise<void> {
  await ipc.termKill(id).catch(() => {
    /* already dead or disconnected */
  });
  disposeInstance(id);
  useTerminalStore.getState().remove(id);
}

/**
 * After a webview reload the backend may hold sessions whose streams were
 * bound to the previous page. Surface them as disconnected instead of
 * silently killing someone's shell.
 */
export async function reconcileSessions(): Promise<void> {
  if (!inDesktopShell) return;
  const backend = await ipc.termList();
  const store = useTerminalStore.getState();
  for (const info of backend) {
    if (!getInstance(info.id)) {
      store.addDisconnected(info);
    }
  }
}
