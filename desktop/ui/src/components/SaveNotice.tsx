import { useSyncExternalStore } from "react";
import type { ManualSave } from "../lib/manualSave";

export default function SaveNotice({ controller, visibleInForm, onRestore }: { controller: ManualSave; visibleInForm: boolean; onRestore: () => void }) {
  const operation = useSyncExternalStore(controller.subscribe, controller.getSnapshot, controller.getSnapshot);
  if (!operation || visibleInForm) return null;
  const terminal = operation.phase === "complete" || operation.phase === "rejected";
  return (
    <aside className="flex flex-wrap items-center gap-3 rounded-md border border-amber-900 p-3 text-sm text-amber-300" aria-label="手入力の保存操作">
      <p>{terminal ? "手入力の保存結果を保持しています。" : "未完了の手入力を保持しています。新しい保存は開始できません。"}</p>
      <button type="button" onClick={onRestore} className="rounded border border-amber-800 px-3 py-1 text-sm">元の保存操作を確認</button>
    </aside>
  );
}
