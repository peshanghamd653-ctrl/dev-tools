import {
  CircleCheck,
  FolderPlus,
  RefreshCw,
  ShieldAlert,
  TriangleAlert,
} from "lucide-react";
import { toast } from "sonner";

import { useGitStore } from "@/features/git/store";
import { useProjects } from "@/features/projects/hooks";
import { useActiveWorkspace } from "@/features/workspaces/hooks";
import {
  isDesktopShell,
  type DependencyCheck,
  type GitCheck,
  type SecretScan,
} from "@/shared/ipc/client";
import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Card, CardContent } from "@/shared/ui/card";
import { useSecurityScan } from "./hooks";

export function SecurityPage() {
  const activeWorkspace = useActiveWorkspace();
  const { data: projects } = useProjects(activeWorkspace?.id);
  const selectedProjectId = useGitStore((s) => s.selectedProjectId);
  const project =
    projects?.find((p) => p.id === selectedProjectId) ?? projects?.[0] ?? null;
  const scan = useSecurityScan();

  if (!isDesktopShell()) {
    return <EmptyCard title="The security center needs the desktop shell" />;
  }

  if (!project) {
    return (
      <EmptyCard title="No project yet">
        Add a project to scan its working tree, its tracked files, and its
        dependency manifests.
      </EmptyCard>
    );
  }

  const report = scan.data;

  return (
    <div className="mx-auto max-w-3xl space-y-4 p-6">
      <div className="flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Security</h1>
          <p className="text-sm text-muted-foreground">
            Git cleanliness, a secret scan, and a dependency vulnerability audit
            for {project.name}.
          </p>
        </div>
        <Button
          size="sm"
          disabled={scan.isPending}
          onClick={() =>
            scan.mutate(project.path, {
              onError: (error) => toast.error(String(error)),
            })
          }
        >
          <RefreshCw
            className={cn("size-4", scan.isPending && "animate-spin")}
          />
          {scan.isPending ? "Scanning…" : report ? "Re-scan" : "Scan"}
        </Button>
      </div>

      {!report && !scan.isPending && (
        <EmptyCard title="No scan yet">
          Click Scan to check the working tree, look for credential-shaped text
          in tracked files, and audit dependencies for known vulnerabilities.
        </EmptyCard>
      )}

      {report && (
        <div className="space-y-3">
          <GitSection check={report.git} />
          <SecretsSection scan={report.secrets} />
          {report.dependencies.map((check) => (
            <DependencySection key={check.ecosystem} check={check} />
          ))}
        </div>
      )}
    </div>
  );
}

function StatusRow({
  ok,
  warn,
  title,
  detail,
}: {
  /** `true` → clean check mark. `false` with `warn` unset → neutral info. */
  ok: boolean;
  warn?: boolean;
  title: string;
  detail?: string;
}) {
  const tone = ok ? "good" : warn ? "warn" : "neutral";
  const Icon = ok ? CircleCheck : warn ? TriangleAlert : ShieldAlert;
  const toneClass = {
    good: "text-emerald-400",
    warn: "text-yellow-300",
    neutral: "text-muted-foreground",
  }[tone];

  return (
    <Card className="gap-0 py-3">
      <CardContent className="flex items-start gap-3 px-4">
        <Icon className={cn("mt-0.5 size-4 shrink-0", toneClass)} />
        <div className="min-w-0 flex-1">
          <p className="text-sm">{title}</p>
          {detail && <p className="text-xs text-muted-foreground">{detail}</p>}
        </div>
      </CardContent>
    </Card>
  );
}

function GitSection({ check }: { check: GitCheck }) {
  if (!check.isRepo) {
    return <StatusRow ok={false} title="Not a git repository" />;
  }
  return check.clean ? (
    <StatusRow ok title="Git working tree clean" />
  ) : (
    <StatusRow
      ok={false}
      warn
      title="Uncommitted changes"
      detail={`${check.changedFiles} file${check.changedFiles === 1 ? "" : "s"} changed`}
    />
  );
}

function SecretsSection({ scan }: { scan: SecretScan }) {
  if (scan.findings.length === 0) {
    return (
      <StatusRow
        ok
        title="No exposed secrets"
        detail={`${scan.filesScanned} file${scan.filesScanned === 1 ? "" : "s"} scanned`}
      />
    );
  }
  return (
    <Card className="gap-0 py-3">
      <CardContent className="space-y-2 px-4">
        <div className="flex items-start gap-3">
          <TriangleAlert className="mt-0.5 size-4 shrink-0 text-yellow-300" />
          <p className="text-sm">
            {scan.findings.length} potential secret
            {scan.findings.length === 1 ? "" : "s"} found
          </p>
        </div>
        <ul className="space-y-1 pl-7">
          {scan.findings.map((finding, i) => (
            <li key={i} className="font-mono text-xs text-muted-foreground">
              {finding.file}:{finding.line} — {finding.kind}
            </li>
          ))}
        </ul>
      </CardContent>
    </Card>
  );
}

function DependencySection({ check }: { check: DependencyCheck }) {
  if (check.status === "ok") {
    return (
      <StatusRow
        ok
        title={`No known ${check.ecosystem} dependency vulnerabilities`}
      />
    );
  }
  if (check.status === "vulnerable") {
    return (
      <StatusRow
        ok={false}
        warn
        title={`${check.vulnerabilityCount} ${check.ecosystem} dependency vulnerabilit${check.vulnerabilityCount === 1 ? "y" : "ies"}`}
      />
    );
  }
  if (check.status === "toolNotInstalled") {
    return (
      <StatusRow
        ok={false}
        title={`${check.ecosystem} audit not run`}
        detail={check.detail ?? undefined}
      />
    );
  }
  if (check.status === "unsupported") {
    return (
      <StatusRow
        ok={false}
        title={`${check.ecosystem} audit not supported yet`}
        detail={check.detail ?? undefined}
      />
    );
  }
  return (
    <StatusRow
      ok={false}
      title={`${check.ecosystem} audit failed`}
      detail={check.detail ?? undefined}
    />
  );
}

function EmptyCard({
  title,
  children,
}: {
  title: string;
  children?: React.ReactNode;
}) {
  return (
    <div className="mx-auto max-w-2xl py-6">
      <Card>
        <CardContent className="flex flex-col items-center gap-2 py-12 text-center">
          <FolderPlus className="size-6 text-muted-foreground" />
          <p className="font-medium">{title}</p>
          {children && (
            <p className="max-w-md text-sm text-muted-foreground">{children}</p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
