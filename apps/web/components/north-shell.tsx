"use client";

import type { ReactNode, SVGProps } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";

type ThemeMode = "system" | "light" | "dark";
type IconName = "board" | "activity" | "repository" | "members";

type NavItem = {
  label: string;
  href: string;
  icon: IconName;
  count?: number;
};

type NavSection = {
  label: string;
  items: NavItem[];
};

const themeModes: ThemeMode[] = ["system", "light", "dark"];
const themeLabels: Record<ThemeMode, string> = {
  system: "跟隨系統",
  light: "淺色",
  dark: "深色",
};

const navSections: NavSection[] = [
  {
    label: "Workspace",
    items: [{ label: "需求", href: "/", icon: "board" }],
  },
  {
    label: "Manage",
    items: [
      { label: "執行狀態", href: "/settings/daemons", icon: "activity" },
      { label: "儲存庫", href: "/settings/repositories", icon: "repository" },
    ],
  },
  {
    label: "System",
    items: [{ label: "成員", href: "/admin/users", icon: "members" }],
  },
];

function NavIcon({ kind }: { kind: IconName }) {
  const props: SVGProps<SVGSVGElement> = {
    "aria-hidden": true,
    fill: "none",
    height: 15,
    stroke: "currentColor",
    strokeLinecap: "round",
    strokeLinejoin: "round",
    strokeWidth: 1.7,
    viewBox: "0 0 24 24",
    width: 15,
  };

  if (kind === "board") {
    return (
      <svg {...props}>
        <rect height="6" rx="1" width="6" x="4" y="4" />
        <rect height="6" rx="1" width="6" x="14" y="4" />
        <rect height="6" rx="1" width="6" x="4" y="14" />
        <rect height="6" rx="1" width="6" x="14" y="14" />
      </svg>
    );
  }

  if (kind === "activity") {
    return (
      <svg {...props}>
        <path d="M4 18h16" />
        <path d="M5 15.5 9 11l3 2.5 6-7" />
        <path d="M16 6.5h2v2" />
      </svg>
    );
  }

  if (kind === "repository") {
    return (
      <svg {...props}>
        <path d="M7 3.5h10v17H7z" />
        <path d="M10 7h4M10 11h4M10 15h2" />
      </svg>
    );
  }

  return (
    <svg {...props}>
      <circle cx="9" cy="8" r="3" />
      <path d="M3.5 20c.7-3.1 2.5-4.5 5.5-4.5s4.8 1.4 5.5 4.5" />
      <path d="M16 5.5a3 3 0 0 1 0 5.8M17 15.8c2 .5 3.2 1.8 3.7 4.2" />
    </svg>
  );
}

function NorthLogo({ compact = false }: { compact?: boolean }) {
  return (
    <div className="north-logo">
      <span aria-hidden="true" className="north-logo-mark">
        N
      </span>
      {!compact && (
        <span className="north-logo-copy">
          <strong>North</strong>
          <span className="north-version">0.1.0</span>
        </span>
      )}
    </div>
  );
}

function ThemeButton() {
  const [mode, setMode] = useState<ThemeMode>("system");

  useEffect(() => {
    const themeWindow = window as Window & {
      __getTheme?: () => ThemeMode;
    };
    const sync = () => {
      const value = themeWindow.__getTheme?.();
      if (value && themeModes.includes(value)) setMode(value);
    };
    sync();
    window.addEventListener("themechange", sync);
    return () => window.removeEventListener("themechange", sync);
  }, []);

  function cycleTheme() {
    const next = themeModes[(themeModes.indexOf(mode) + 1) % themeModes.length];
    const themeWindow = window as Window & {
      __setTheme?: (value: ThemeMode) => void;
    };
    themeWindow.__setTheme?.(next);
    setMode(next);
  }

  return (
    <button
      aria-label="切換主題"
      className="north-theme-button"
      title={`主題：${themeLabels[mode]}`}
      type="button"
      onClick={cycleTheme}
    >
      <svg
        aria-hidden="true"
        fill="none"
        height="15"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="1.7"
        viewBox="0 0 24 24"
        width="15"
      >
        <circle cx="12" cy="12" r="4" />
        <path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4" />
      </svg>
      <span>主題 · {themeLabels[mode]}</span>
    </button>
  );
}

function Sidebar({
  open,
  pathname,
  onNavigate,
}: {
  open: boolean;
  pathname: string;
  onNavigate: () => void;
}) {
  return (
    <aside
      aria-label="主導覽"
      className={`north-sidebar${open ? " is-open" : ""}`}
      id="app-sidebar"
    >
      <div className="north-sidebar-head">
        <NorthLogo />
      </div>
      <nav aria-label="主導覽" className="north-sidebar-nav">
        {navSections.map((section) => (
          <div key={section.label}>
            <p className="north-nav-section-label">{section.label}</p>
            {section.items.map((item) => {
              const active =
                item.href === "/"
                  ? pathname === "/" || pathname.startsWith("/requirements/")
                  : pathname.startsWith(item.href);
              return (
                <Link
                  aria-current={active ? "page" : undefined}
                  className={`north-side-link${active ? " is-active" : ""}`}
                  href={item.href}
                  key={item.href}
                  onClick={onNavigate}
                >
                  <NavIcon kind={item.icon} />
                  <span>{item.label}</span>
                  {item.count !== undefined && (
                    <span className="north-nav-count">{item.count}</span>
                  )}
                </Link>
              );
            })}
          </div>
        ))}
      </nav>
      <div className="north-sidebar-foot">
        <ThemeButton />
        <div className="north-sse-inline" role="status">
          <span aria-hidden="true" className="north-sse-dot" />
          <span>已連線 · 自動更新</span>
        </div>
        <div className="north-user-card">
          <span aria-hidden="true" className="north-avatar">
            AC
          </span>
          <span className="north-user-copy">
            <strong>管理員</strong>
            <span>admin@north.local</span>
          </span>
          <span className="north-user-role">Owner</span>
        </div>
      </div>
    </aside>
  );
}

export function PageHeader({
  actions,
  description,
  eyebrow,
  title,
}: {
  actions?: ReactNode;
  description?: string;
  eyebrow?: string;
  title: string;
}) {
  return (
    <header className="north-page-head">
      <div className="min-w-0">
        {eyebrow && <p className="north-eyebrow">{eyebrow}</p>}
        <h1>{title}</h1>
        {description && <p className="north-page-sub">{description}</p>}
      </div>
      {actions && <div className="north-head-actions">{actions}</div>}
    </header>
  );
}

export function NorthShell({ children }: { children: ReactNode }) {
  const pathname = usePathname();
  const [sidebarOpen, setSidebarOpen] = useState(false);

  useEffect(() => {
    if (!sidebarOpen) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setSidebarOpen(false);
    };
    const previousOverflow = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.body.style.overflow = previousOverflow;
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [sidebarOpen]);

  return (
    <div className="north-app">
      <Sidebar
        onNavigate={() => setSidebarOpen(false)}
        open={sidebarOpen}
        pathname={pathname}
      />
      <button
        aria-hidden={!sidebarOpen}
        aria-label="關閉導覽"
        className={`north-sidebar-overlay${sidebarOpen ? " is-open" : ""}`}
        tabIndex={sidebarOpen ? 0 : -1}
        type="button"
        onClick={() => setSidebarOpen(false)}
      />
      <div className="north-main">
        <div className="north-mobile-topbar">
          <button
            aria-controls="app-sidebar"
            aria-expanded={sidebarOpen}
            aria-label="開啟導覽"
            className="north-hamburger"
            type="button"
            onClick={() => setSidebarOpen(true)}
          >
            <svg
              aria-hidden="true"
              fill="none"
              height="18"
              stroke="currentColor"
              strokeLinecap="round"
              strokeWidth="1.8"
              viewBox="0 0 24 24"
              width="18"
            >
              <path d="M4 7h16M4 12h16M4 17h16" />
            </svg>
          </button>
          <NorthLogo compact />
          <span className="north-mobile-version">0.1.0</span>
        </div>
        {children}
      </div>
    </div>
  );
}
