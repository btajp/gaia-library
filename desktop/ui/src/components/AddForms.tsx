import { useState, useSyncExternalStore, type FormEvent } from "react";
import { MANUAL_FORMS } from "../forms/manualFields";
import { buildManualIntent, valuesFromIntent, type FormValues } from "../forms/manualPayload";
import { canCorrectSave, type ManualSave } from "../lib/manualSave";
import type { ManualTarget } from "../proposalTypes";
import type { OpenDetail } from "../types";
import ManualField from "./ManualField";
import ManualSaveStatus from "./ManualSaveStatus";
import OperationError from "./OperationError";

type Props = { scope: string; controller: ManualSave; openDetail: OpenDetail; restoreOperation: () => void; showProposals: () => void };

export default function AddForms({ scope, controller, openDetail, restoreOperation, showProposals }: Props) {
  const operation = useSyncExternalStore(controller.subscribe, controller.getSnapshot, controller.getSnapshot);
  const initial = operation?.input.scope === scope ? operation.input : null;
  const [target, setTarget] = useState<ManualTarget>(() => initial?.target_type ?? "person");
  const [values, setValues] = useState<FormValues>(() => initial ? valuesFromIntent(initial) : {});
  const [factKind, setFactKind] = useState(() => initial?.kind ?? "");
  const [error, setError] = useState<unknown>(null);

  if (operation && operation.input.scope !== scope) {
    return (
      <section className="space-y-4">
        <h2 className="text-xl font-semibold">手入力</h2>
        <p className="text-sm text-amber-300">別の scope の保存操作を保持しています。重複保存を防ぐため、新規入力はできません。</p>
        <button type="button" onClick={restoreOperation} className="rounded border border-neutral-600 px-3 py-2 text-sm">元の scope の保存操作を確認</button>
      </section>
    );
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (controller.getSnapshot()) return;
    setError(null);
    try {
      await controller.start(buildManualIntent(target, values, factKind, scope));
    } catch (cause) {
      setError(cause);
    }
  }

  function clearOperation() {
    const previous = controller.getSnapshot();
    if (!previous || !controller.clearAfterReview()) return;
    if (!canCorrectSave(previous)) {
      setValues({});
      setFactKind("");
    }
    setError(null);
  }

  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-semibold">手入力</h2>
        <p className="mt-2 text-sm leading-6 text-neutral-400">新規追加専用です。保存時は提案を作成し、このアプリの human クライアントで承認まで実行します。</p>
        <p className="mt-1 text-xs text-neutral-400">任意項目は空欄なら送信しません。日付・状態・種別に自動の初期値は入れません。</p>
      </div>
      {operation && <ManualSaveStatus operation={operation} controller={controller} openDetail={openDetail} showProposals={showProposals} onClear={clearOperation} />}
      {!scope && <p role="alert" className="text-sm text-amber-300">保存先の scope を入力するか、クライアントの既定 scope の確認を待ってください。</p>}
      <form onSubmit={submit} className="space-y-5">
        <fieldset disabled={operation !== null || !scope} className="space-y-5 disabled:opacity-60">
          <div>
            <label htmlFor="manual-type" className="block text-sm font-medium">追加する種別</label>
            <select id="manual-type" value={target} onChange={(event) => { setTarget(event.target.value as ManualTarget); setValues({}); setFactKind(""); setError(null); }} className="mt-2 rounded-md border border-neutral-700 bg-neutral-950 px-3 py-2 text-sm">
              {Object.entries(MANUAL_FORMS).map(([key, form]) => <option key={key} value={key}>{form.label}</option>)}
            </select>
          </div>
          {MANUAL_FORMS[target].fields.map((field) => <ManualField key={`${target}:${field.key}`} field={field} value={values[field.key] ?? ""} disabled={operation !== null || !scope} onChange={(value) => setValues((current) => ({ ...current, [field.key]: value }))} />)}
          {target === "fact" ? (
            <fieldset className="space-y-2">
              <legend className="text-sm font-medium">kind（必須）</legend>
              <label className="mr-4 inline-flex items-center gap-2 text-sm"><input type="radio" name="fact-kind" value="fact" required checked={factKind === "fact"} onChange={() => setFactKind("fact")} />fact（事実）</label>
              <label className="inline-flex items-center gap-2 text-sm"><input type="radio" name="fact-kind" value="inference" required checked={factKind === "inference"} onChange={() => setFactKind("inference")} />inference（推測）</label>
            </fieldset>
          ) : <p className="text-xs text-neutral-400">この種別は kind = fact（事実）として提案します。</p>}
          <OperationError error={error} />
          <button type="submit" className="rounded-md bg-neutral-100 px-4 py-2 text-sm font-medium text-neutral-950 disabled:opacity-50" disabled={operation !== null || !scope}>提案・承認して保存</button>
        </fieldset>
      </form>
    </div>
  );
}
