import type { ReactNode } from "react";

export const inputClass = "mt-2 w-full rounded-md border border-neutral-700 bg-neutral-950 px-3 py-2 text-sm disabled:opacity-50";
export const buttonClass = "rounded-md border border-neutral-600 px-3 py-2 text-sm hover:bg-neutral-800 disabled:cursor-not-allowed disabled:opacity-50";
export const primaryClass = "rounded-md bg-neutral-100 px-4 py-2 text-sm font-medium text-neutral-950 hover:bg-white disabled:cursor-not-allowed disabled:opacity-50";

export function SettingsSection({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  return (
    <section aria-labelledby={id} className="space-y-4 rounded-lg border border-neutral-700 p-5">
      <h3 id={id} className="text-lg font-semibold">{title}</h3>
      {children}
    </section>
  );
}

export function SettingsError({ children }: { children: ReactNode }) {
  return <p role="alert" className="break-words text-sm leading-6 text-red-300">{children}</p>;
}

export function ReloadButton({ loading, refresh }: { loading: boolean; refresh: () => void }) {
  return <button type="button" onClick={refresh} disabled={loading} className={buttonClass}>{loading ? "読込中…" : "再読込"}</button>;
}
