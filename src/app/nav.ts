import {
  Container,
  Database,
  FolderKanban,
  FolderTree,
  GitBranch,
  LayoutDashboard,
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
  { to: "/settings", label: "Settings", icon: Settings, shortcut: "Ctrl+," },
];

export interface UpcomingItem {
  label: string;
  icon: LucideIcon;
  milestone: string;
}

/** Planned modules, shown disabled so the roadmap is visible but honest. */
export const upcomingNav: UpcomingItem[] = [
  { label: "Docker", icon: Container, milestone: "M3" },
  { label: "API Client", icon: Send, milestone: "M3" },
  { label: "Database", icon: Database, milestone: "M3" },
];

export function navItemForPath(path: string): NavItem | undefined {
  return primaryNav.find((item) => item.to === path);
}
