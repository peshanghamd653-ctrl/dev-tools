import { describe, expect, it } from "vitest";

import type { DeployProject, Deployment } from "@/shared/ipc/client";
import {
  classifyDeployError,
  commitSummary,
  deploymentHref,
  deployState,
  deployStateLabel,
  displayUrl,
  sortDeployments,
  sortProjects,
  targetLabel,
  timeAgo,
} from "./state";

function deployment(over: Partial<Deployment> = {}): Deployment {
  return {
    id: "dpl_1",
    name: "web",
    url: "web-a1b2c3.vercel.app",
    state: "READY",
    target: "production",
    createdAt: 1_700_000_000_000,
    commitMessage: "fix: retry failed webhooks",
    ...over,
  };
}

function project(over: Partial<DeployProject> = {}): DeployProject {
  return {
    id: "prj_1",
    name: "web",
    framework: "nextjs",
    updatedAt: 1_700_000_000_000,
    ...over,
  };
}

describe("deployState", () => {
  it("maps the documented Vercel states", () => {
    expect(deployState("READY")).toBe("ready");
    expect(deployState("ERROR")).toBe("error");
    expect(deployState("BUILDING")).toBe("building");
    expect(deployState("QUEUED")).toBe("queued");
    expect(deployState("CANCELED")).toBe("canceled");
  });

  it("accepts both spellings of canceled", () => {
    expect(deployState("CANCELLED")).toBe("canceled");
  });

  it("tolerates casing and whitespace", () => {
    expect(deployState(" ready ")).toBe("ready");
  });

  it("falls back to unknown instead of throwing", () => {
    expect(deployState("SOMETHING_NEW")).toBe("unknown");
    expect(deployState("")).toBe("unknown");
  });
});

describe("deployStateLabel", () => {
  it("names the known states", () => {
    expect(deployStateLabel("READY")).toBe("Ready");
    expect(deployStateLabel("BUILDING")).toBe("Building");
  });

  it("renders an unseen state readably rather than blank", () => {
    expect(deployStateLabel("SOME_NEW_STATE")).toBe("Some new state");
    expect(deployStateLabel("INITIALIZING")).toBe("Initializing");
  });

  it("never produces an empty badge", () => {
    expect(deployStateLabel("")).toBe("Unknown");
    expect(deployStateLabel("   ")).toBe("Unknown");
  });
});

describe("deploymentHref", () => {
  it("adds the scheme Vercel omits", () => {
    expect(deploymentHref("web-a1b2c3.vercel.app")).toBe(
      "https://web-a1b2c3.vercel.app/",
    );
  });

  it("keeps an absolute http(s) URL", () => {
    expect(deploymentHref("http://localhost:3000/")).toBe(
      "http://localhost:3000/",
    );
  });

  it("refuses anything that isn't http(s)", () => {
    expect(deploymentHref("file:///etc/passwd")).toBeNull();
    expect(deploymentHref("javascript:alert(1)")).toBeNull();
  });

  it("refuses a URL carrying embedded credentials", () => {
    // `mailto:` collapses into userinfo once a scheme is prepended, and the
    // same shape is how a link disguises its real host.
    expect(deploymentHref("mailto:someone@example.com")).toBeNull();
    expect(deploymentHref("https://vercel.app:x@evil.example")).toBeNull();
  });

  it("refuses an empty url", () => {
    expect(deploymentHref("")).toBeNull();
    expect(deploymentHref("   ")).toBeNull();
  });
});

describe("displayUrl", () => {
  it("strips the scheme and a trailing slash", () => {
    expect(displayUrl("https://web.vercel.app/")).toBe("web.vercel.app");
    expect(displayUrl("web.vercel.app")).toBe("web.vercel.app");
  });
});

describe("commitSummary", () => {
  it("returns null when there is no message", () => {
    expect(commitSummary(null)).toBeNull();
    expect(commitSummary("")).toBeNull();
    expect(commitSummary("  \n  ")).toBeNull();
  });

  it("keeps only the subject line", () => {
    expect(commitSummary("feat: thing\n\nlong body here")).toBe("feat: thing");
  });
});

describe("targetLabel", () => {
  it("reads a null target as preview", () => {
    expect(targetLabel(null)).toBe("Preview");
    expect(targetLabel("  ")).toBe("Preview");
  });

  it("title-cases whatever Vercel sent", () => {
    expect(targetLabel("production")).toBe("Production");
    expect(targetLabel("STAGING")).toBe("Staging");
  });
});

describe("timeAgo", () => {
  const now = 1_700_000_000_000;

  it("rounds up the ladder", () => {
    expect(timeAgo(now - 30_000, now)).toBe("just now");
    expect(timeAgo(now - 5 * 60_000, now)).toBe("5m ago");
    expect(timeAgo(now - 3 * 3_600_000, now)).toBe("3h ago");
    expect(timeAgo(now - 2 * 86_400_000, now)).toBe("2d ago");
  });

  it("does not render clock skew as negative time", () => {
    expect(timeAgo(now + 10_000, now)).toBe("just now");
  });

  it("survives a nonsense timestamp", () => {
    expect(timeAgo(Number.NaN, now)).toBe("—");
  });
});

describe("sorting", () => {
  it("puts the newest project first without mutating the source", () => {
    const source = [
      project({ id: "old", updatedAt: 1 }),
      project({ id: "new", updatedAt: 3 }),
      project({ id: "mid", updatedAt: 2 }),
    ];
    expect(sortProjects(source).map((p) => p.id)).toEqual([
      "new",
      "mid",
      "old",
    ]);
    expect(source.map((p) => p.id)).toEqual(["old", "new", "mid"]);
  });

  it("puts the newest deployment first without mutating the source", () => {
    const source = [
      deployment({ id: "a", createdAt: 1 }),
      deployment({ id: "c", createdAt: 3 }),
      deployment({ id: "b", createdAt: 2 }),
    ];
    expect(sortDeployments(source).map((d) => d.id)).toEqual(["c", "b", "a"]);
    expect(source.map((d) => d.id)).toEqual(["a", "c", "b"]);
  });
});

describe("classifyDeployError", () => {
  // These two are the exact `Display` strings of DeployError::Auth and
  // DeployError::NotConfigured, which is what Tauri hands the webview.
  it("reads the backend's rejected-token error", () => {
    expect(classifyDeployError("Vercel rejected the token (HTTP 401)")).toBe(
      "auth",
    );
    expect(classifyDeployError("Vercel rejected the token (HTTP 403)")).toBe(
      "auth",
    );
  });

  it("reads the backend's missing-token error as its own case", () => {
    expect(classifyDeployError("no Vercel token configured")).toBe(
      "unconfigured",
    );
  });

  it("does not confuse a rejected token with a missing one", () => {
    expect(classifyDeployError("Vercel rejected the token (HTTP 401)")).not.toBe(
      "unconfigured",
    );
    expect(classifyDeployError("no Vercel token configured")).not.toBe("auth");
  });

  it("tolerates a reworded message", () => {
    expect(classifyDeployError(new Error("401 Unauthorized"))).toBe("auth");
    expect(classifyDeployError("forbidden")).toBe("auth");
    expect(classifyDeployError("missing token")).toBe("unconfigured");
  });

  it("does not read a status code out of another error's response body", () => {
    expect(
      classifyDeployError('Vercel API error (HTTP 500): {"hint":"403 earlier"}'),
    ).toBe("other");
    expect(classifyDeployError("request failed: dpl_x401y not found")).toBe(
      "other",
    );
  });

  it("falls back to other for everything else", () => {
    expect(classifyDeployError(new Error("request failed: dns error"))).toBe(
      "other",
    );
    expect(classifyDeployError(undefined)).toBe("other");
  });
});
