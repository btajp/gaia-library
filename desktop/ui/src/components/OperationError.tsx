import { errorMessage, GaiaError } from "../api";

export default function OperationError({ error }: { error: unknown }) {
  if (error === null || error === undefined) return null;
  return (
    <div className="space-y-2">
      <p role="alert" className="break-words text-sm text-red-300">{errorMessage(error)}</p>
      {error instanceof GaiaError && error.details !== undefined && (
        <details className="text-xs text-neutral-400">
          <summary className="cursor-pointer">エラー詳細</summary>
          <pre className="mt-2 overflow-auto whitespace-pre-wrap break-words rounded bg-neutral-950 p-3">{JSON.stringify(error.details, null, 2)}</pre>
        </details>
      )}
    </div>
  );
}
