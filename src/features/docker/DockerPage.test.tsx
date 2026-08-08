/**
 * Docker not running is the *normal* state for most sessions, not an error, and
 * the whole point of the `Unavailable`/`Api` split in the backend is that the
 * UI can degrade instead of showing a stack trace. These tests pin both halves
 * of that split, including the boundary: an ordinary API failure must not be
 * mistaken for "the daemon is off".
 */
import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/shared/ipc/client", async () => {
  const { createClientMock } = await import("@/shared/test/ipc");
  return createClientMock();
});

import {
  ipc,
  type DockerContainer,
  type DockerImage,
} from "@/shared/ipc/client";
import { resetClientMock, setDesktopShell } from "@/shared/test/ipc";
import { renderWithClient } from "@/shared/test/render";
import { DockerPage } from "./DockerPage";

/** What `docker_ping` rejects with when the daemon socket refuses a connection. */
const UNAVAILABLE =
  "unavailable: error trying to connect: The system cannot find the file specified.";

function container(over: Partial<DockerContainer> = {}): DockerContainer {
  return {
    id: "abc123",
    name: "web",
    image: "nginx:latest",
    state: "running",
    status: "Up 2 hours",
    ports: ["0.0.0.0:8080→80/tcp"],
    created: 1_700_000_000,
    ...over,
  };
}

const IMAGE: DockerImage = {
  id: "sha256:deadbeef",
  tags: ["nginx:latest"],
  size: 142 * 1024 * 1024,
  created: 1_700_000_000,
};

beforeEach(resetClientMock);
afterEach(cleanup);

describe("DockerPage degraded states", () => {
  it("explains itself outside the desktop shell without calling IPC", () => {
    setDesktopShell(false);
    renderWithClient(<DockerPage />);

    expect(screen.getByText("Docker needs the desktop shell")).toBeInTheDocument();
    expect(ipc.dockerPing).not.toHaveBeenCalled();
  });

  it("tells the user to start Docker rather than surfacing the socket error", async () => {
    vi.mocked(ipc.dockerPing).mockRejectedValue(UNAVAILABLE);
    renderWithClient(<DockerPage />);

    expect(await screen.findByText("Docker isn't running")).toBeInTheDocument();
    expect(
      screen.getByText(/Start Docker Desktop, then this page will connect/),
    ).toBeInTheDocument();
    expect(screen.queryByText(UNAVAILABLE)).not.toBeInTheDocument();
  });

  it("does not list containers while the daemon is unreachable", async () => {
    vi.mocked(ipc.dockerPing).mockRejectedValue(UNAVAILABLE);
    renderWithClient(<DockerPage />);

    await screen.findByText("Docker isn't running");
    expect(ipc.dockerContainers).not.toHaveBeenCalled();
    expect(ipc.dockerImages).not.toHaveBeenCalled();
  });

  it("keeps an ordinary API failure out of the 'not running' state", async () => {
    vi.mocked(ipc.dockerPing).mockRejectedValue("permission denied on /var/run/docker.sock");
    renderWithClient(<DockerPage />);

    expect(
      await screen.findByRole("heading", { name: "Docker" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Docker isn't running")).not.toBeInTheDocument();
  });

  /**
   * The other half of that split used to render nothing at all: an
   * unclassified ping failure left an empty page under a header that said
   * "Connecting…" and never stopped saying it.
   */
  it("names the failure instead of leaving the page blank", async () => {
    vi.mocked(ipc.dockerPing).mockRejectedValue("permission denied on /var/run/docker.sock");
    renderWithClient(<DockerPage />);

    expect(await screen.findByText("Couldn't reach Docker")).toBeInTheDocument();
    expect(
      screen.getByText("permission denied on /var/run/docker.sock"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Connecting/)).not.toBeInTheDocument();
  });

  it("says it is still looking while the ping is in flight", async () => {
    vi.mocked(ipc.dockerPing).mockReturnValue(new Promise(() => {}));
    renderWithClient(<DockerPage />);

    expect(await screen.findByText("Looking for Docker")).toBeInTheDocument();
    expect(ipc.dockerContainers).not.toHaveBeenCalled();
  });
});

describe("DockerPage container list", () => {
  beforeEach(() => {
    vi.mocked(ipc.dockerPing).mockResolvedValue("Docker 24.0.7");
    vi.mocked(ipc.dockerImages).mockResolvedValue([]);
  });

  it("says the list is empty instead of rendering nothing", async () => {
    vi.mocked(ipc.dockerContainers).mockResolvedValue([]);
    renderWithClient(<DockerPage />);

    expect(await screen.findByText("No containers")).toBeInTheDocument();
  });

  it("offers stop and restart for a running container, start for a stopped one", async () => {
    vi.mocked(ipc.dockerContainers).mockResolvedValue([
      container(),
      container({ id: "def456", name: "worker", state: "exited", status: "Exited (0)" }),
    ]);
    renderWithClient(<DockerPage />);

    await screen.findByText("web");
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Restart" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start" })).toBeInTheDocument();
    // One of each: the running row must not also offer Start.
    expect(screen.getAllByRole("button", { name: "Start" })).toHaveLength(1);
    expect(screen.getAllByRole("button", { name: "Stop" })).toHaveLength(1);
  });

  it("acts on the container the button belongs to", async () => {
    vi.mocked(ipc.dockerContainers).mockResolvedValue([container()]);
    vi.mocked(ipc.dockerStop).mockResolvedValue(undefined);
    renderWithClient(<DockerPage />);

    await screen.findByText("web");
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));

    await waitFor(() => expect(ipc.dockerStop).toHaveBeenCalledWith("abc123"));
  });

  it("counts what each tab holds so the user can see it without switching", async () => {
    vi.mocked(ipc.dockerContainers).mockResolvedValue([container()]);
    vi.mocked(ipc.dockerImages).mockResolvedValue([IMAGE, { ...IMAGE, id: "sha256:cafe" }]);
    renderWithClient(<DockerPage />);

    expect(
      await screen.findByRole("button", { name: "Containers (1)" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByRole("button", { name: "Images (2)" }),
    ).toBeInTheDocument();
  });

  it("switches to images, falling back to a placeholder for untagged ones", async () => {
    vi.mocked(ipc.dockerContainers).mockResolvedValue([container()]);
    vi.mocked(ipc.dockerImages).mockResolvedValue([
      IMAGE,
      { ...IMAGE, id: "sha256:cafe", tags: [] },
    ]);
    renderWithClient(<DockerPage />);

    fireEvent.click(await screen.findByRole("button", { name: "Images (2)" }));

    expect(screen.getByText("nginx:latest")).toBeInTheDocument();
    expect(screen.getByText("(untagged)")).toBeInTheDocument();
    expect(screen.queryByText("web")).not.toBeInTheDocument();
  });
});
