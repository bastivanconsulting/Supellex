"use client";

import type { ReactNode } from "react";

/* Un seul jeu d'icônes, sobre : SVG inline, trait 1,7, 24 px, couleur héritée. */
const ICONS = {
  dashboard: "M4 4h7v7H4zM13 4h7v4h-7zM13 10h7v10h-7zM4 13h7v7H4z",
  assets: "M3 8l9-5 9 5v9l-9 5-9-5zM3 8l9 5 9-5M12 13v8",
  ipam: "M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM3 12h18M12 3c3 3.5 3 14 0 18-3-4-3-14.5 0-18z",
  vlans: "M4 5h9l7 7-7 7-7-7zM8.5 9.5h.01",
  people: "M9 11a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zM2.5 20c.8-3.2 3.4-5 6.5-5s5.7 1.8 6.5 5M16 4.6a3.5 3.5 0 0 1 0 6.8M18.5 15.4c1.7.7 2.8 2.2 3 4.6",
  locations: "M12 21s-7-5.5-7-11a7 7 0 1 1 14 0c0 5.5-7 11-7 11zM12 12.5a2.5 2.5 0 1 0 0-5 2.5 2.5 0 0 0 0 5z",
  audit: "M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01",
  logout: "M14 4h5v16h-5M10 8l4 4-4 4M14 12H3",
  search: "M10.5 18a7.5 7.5 0 1 0 0-15 7.5 7.5 0 0 0 0 15zM21 21l-5.8-5.8",
  plus: "M12 5v14M5 12h14",
  x: "M6 6l12 12M18 6L6 18",
  device: "M4 4h16v6H4zM4 14h16v6H4zM7.5 7h.01M7.5 17h.01M11 7h5M11 17h5",
  bolt: "M13 2L4 14h6l-1 8 9-12h-6z",
  key: "M7 14a4 4 0 1 0 0-8 4 4 0 0 0 0 8zM10 11l10 10M17 18l2.5-2.5M14 15l2-2",
  edit: "M4 20h4L18.5 9.5a2.1 2.1 0 0 0-3-3L5 17v3zM13.5 6.5l3 3",
  trash: "M4 7h16M9 7V4h6v3M6 7l1 14h10l1-14M10 11v6M14 11v6",
  upload: "M12 16V4M6 10l6-6 6 6M4 20h16",
  warning: "M12 3L2 20h20L12 3zM12 10v4M12 17.5h.01",
} as const;

export function Icon({
  name,
  className = "h-4 w-4",
}: {
  name: keyof typeof ICONS;
  className?: string;
}) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      <path d={ICONS[name]} />
    </svg>
  );
}

/// En-tête de page : titre en serif éditorial (sentence case), actions à droite.
export function PageTitle({
  title,
  children,
}: {
  title: string;
  children?: ReactNode;
}) {
  return (
    <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
      <h1 className="font-display text-3xl leading-tight">{title}</h1>
      {children}
    </div>
  );
}

export function Spinner() {
  return (
    <div className="flex items-center gap-2 text-sm text-muted">
      <span className="h-4 w-4 animate-spin rounded-full border-2 border-line border-t-accent" />
      chargement…
    </div>
  );
}

export function ErrorBox({ message }: { message: string }) {
  return (
    <div className="rounded-md border border-accent/40 bg-accent/10 px-4 py-3 text-sm text-accent">
      {message}
    </div>
  );
}

export function Badge({ children, tone }: { children: ReactNode; tone?: string }) {
  const tones: Record<string, string> = {
    green: "bg-ok/10 text-ok border-ok/30",
    yellow: "bg-warn/10 text-warn border-warn/30",
    red: "bg-accent/10 text-accent border-accent/30",
    blue: "bg-info/10 text-info border-info/30",
    gray: "bg-line/60 text-muted border-line-strong",
  };
  return (
    <span
      className={`inline-block whitespace-nowrap rounded border px-2 py-0.5 text-xs ${tones[tone ?? "gray"]}`}
    >
      {children}
    </span>
  );
}

export function Table({
  columns,
  rows,
}: {
  columns: string[];
  rows: ReactNode[][];
}) {
  return (
    <div className="overflow-x-auto rounded-lg border border-line bg-surface">
      <table className="w-full text-left text-sm tabular-nums">
        <thead className="bg-canvas text-muted">
          <tr>
            {columns.map((c) => (
              <th key={c} className="px-4 py-2 font-medium">
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row, i) => (
            <tr key={i} className="border-t border-line/70 hover:bg-canvas/70">
              {row.map((cell, j) => (
                <td key={j} className="px-4 py-2 align-top">
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function Button({
  children,
  onClick,
  disabled,
  variant = "primary",
  type = "button",
  size = "md",
  className = "",
}: {
  children: ReactNode;
  onClick?: () => void;
  disabled?: boolean;
  variant?: "primary" | "ghost" | "danger";
  type?: "button" | "submit";
  size?: "sm" | "md";
  className?: string;
}) {
  const variants: Record<string, string> = {
    primary: "bg-accent text-white hover:bg-accent-hover disabled:opacity-50",
    ghost:
      "border border-line-strong bg-surface text-muted hover:bg-canvas disabled:opacity-50",
    danger:
      "border border-accent/40 bg-accent/10 text-accent hover:bg-accent/20 disabled:opacity-50",
  };
  const sizes: Record<string, string> = {
    sm: "px-2 py-1 text-xs gap-1",
    md: "px-3 py-2 text-sm gap-1.5",
  };
  return (
    <button
      type={type}
      onClick={onClick}
      disabled={disabled}
      className={`inline-flex items-center rounded-md font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 disabled:opacity-50 ${sizes[size]} ${variants[variant]} ${className}`}
    >
      {children}
    </button>
  );
}

export function Input({
  value,
  onChange,
  placeholder,
  type = "text",
  className = "",
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  type?: string;
  className?: string;
}) {
  return (
    <input
      type={type}
      value={value}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value)}
      className={`rounded-md border border-line-strong bg-surface px-3 py-2 text-sm text-ink outline-none focus:border-accent focus-visible:ring-2 focus-visible:ring-accent/30 ${className}`}
    />
  );
}

export function Select({
  value,
  onChange,
  children,
  className = "",
}: {
  value: string;
  onChange: (v: string) => void;
  children: ReactNode;
  className?: string;
}) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value)}
      className={`rounded-md border border-line-strong bg-surface px-3 py-2 text-sm text-ink outline-none focus:border-accent focus-visible:ring-2 focus-visible:ring-accent/30 ${className}`}
    >
      {children}
    </select>
  );
}

/// Barre d'utilisation façon PHPIPAM (couleurs par taux).
export function UtilBar({ pct }: { pct: number }) {
  const color = pct >= 90 ? "bg-accent" : pct >= 70 ? "bg-warn" : "bg-ok";
  return (
    <div className="flex items-center gap-2">
      <div className="h-2 w-28 overflow-hidden rounded-full bg-line">
        <div className={`h-full ${color}`} style={{ width: `${Math.min(100, pct)}%` }} />
      </div>
      <span className="text-xs text-muted">{pct.toFixed(1).replace(".", ",")}&nbsp;%</span>
    </div>
  );
}

export function Modal({
  title,
  onClose,
  children,
  wide = false,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  wide?: boolean;
}) {
  return (
    <div
      className="fixed inset-0 z-50 overflow-y-auto bg-ink/30 p-4"
      onClick={onClose}
    >
      {/* m-auto : centrée verticalement si courte, scrollable dans le overlay si longue */}
      <div
        className={`m-auto w-full ${wide ? "max-w-2xl" : "max-w-lg"} max-h-[calc(100vh-2rem)] overflow-y-auto rounded-xl border border-line bg-surface p-6 shadow-lg`}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-semibold">{title}</h2>
          <button
            onClick={onClose}
            className="text-faint hover:text-ink focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/40 rounded"
            aria-label="fermer"
          >
            <Icon name="x" />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}

export function Field({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <label className="mb-3 block text-sm">
      <span className="mb-1 block text-muted">{label}</span>
      {children}
    </label>
  );
}

/// Vraie modale de confirmation (remplace les confirm() natifs du navigateur).
export function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel = "Confirmer",
  loading = false,
  onCancel,
  onConfirm,
}: {
  open: boolean;
  title: string;
  message: string;
  confirmLabel?: string;
  loading?: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  if (!open) return null;
  return (
    <Modal title={title} onClose={onCancel}>
      <div className="flex items-start gap-3">
        <span className="mt-0.5 text-accent">
          <Icon name="warning" className="h-5 w-5" />
        </span>
        <p className="text-sm text-muted">{message}</p>
      </div>
      <div className="mt-5 flex justify-end gap-2">
        <Button variant="ghost" onClick={onCancel} disabled={loading}>
          Annuler
        </Button>
        <Button variant="danger" onClick={onConfirm} disabled={loading}>
          {loading ? "suppression…" : confirmLabel}
        </Button>
      </div>
    </Modal>
  );
}
