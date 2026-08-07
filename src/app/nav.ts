import {
  Activity,
  Code,
  Container,
  Database,
  FolderKanban,
  FolderTree,
  GitBranch,
  LayoutDashboard,
  Rocket,
  Send,
  Settings,
  Sparkles,
  Terminal,
  type LucideIcon,
} from "lucide-react";

export interface NavItem {
  to: string;
  label: string;
  icon: LucideIcon;
  shortcut?: string;
}

export const primaryNav: NavItem[] = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard, shortcut: "Ctrl+1" },
  { to: "/projects", label: "Projects", icon: FolderKanban, shortcut: "Ctrl+2" },
  { to: "/terminal", label: "Terminal", icon: Terminal, shortcut: "Ctrl+3" },
  { to: "/git", label: "Git", icon: GitBranch, shortcut: "Ctrl+4" },
  { to: "/ai", label: "AI Assistant", icon: Sparkles, shortcut: "Ctrl+5" },
  { to: "/files", label: "Files", icon: FolderTree, shortcut: "Ctrl+6" },
  { to: "/docker", label: "Docker", icon: Container, shortcut: "Ctrl+7" },
  { to: "/api", label: "API Client", icon: Send, shortcut: "Ctrl+8" },
  { to: "/database", label: "Database", icon: Database, shortcut: "Ctrl+9" },
  { to: "/monitors", label: "Monitors", icon: Activity, shortcut: "Ctrl+0" },
  // The digits ran out at Monitors; Deployments takes a shift-modified key.
  { to: "/deploy", label: "Deployments", icon: Rocket, shortcut: "Ctrl+Shift+D" },
  { to: "/snippets", label: "Snippets", icon: Code, shortcut: "Ctrl+Shift+N" },
  { to: "/settings", label: "Settings", icon: Settings, shortcut: "Ctrl+," },
];

export interface UpcomingItem {
  label: string;
  icon: LucideIcon;
  milestone: string;
}

/**
 * Planned modules, shown disabled so the roadmap is visible but honest.
 * Empty as of M3 — every planned module has shipped. The sidebar hides the
 * whole "Coming soon" section while this is empty.
 */
export const upcomingNav: UpcomingItem[] = [];

export function navItemForPath(path: string): NavItem | undefined {
  return primaryNav.find((item) => item.to === path);
}
