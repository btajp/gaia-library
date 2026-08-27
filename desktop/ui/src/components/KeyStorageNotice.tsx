import type { KeyStorage } from "../api";

export default function KeyStorageNotice({ storage }: { storage: KeyStorage }) {
  return (
    <div className="space-y-2 text-sm leading-6">
      {storage.location === "keychain" && (
        <p className="text-neutral-300">キーを macOS の Keychain に保管しました。設定画面の明示操作で、接続設定を再表示できます。</p>
      )}
      {storage.location === "file" && (
        <p className="text-amber-200">Keychain を利用できなかったため、キーを権限 0600（所有者のみ読み書き可）のローカルファイルへ保管しました。接続設定は設定画面で再表示できます。</p>
      )}
      {storage.location === null && (
        <p role="alert" className="text-amber-200">キーの発行は完了していますが、平文を保管できませんでした。閉じる前に安全な場所へコピーしてください。このキーは後から再表示できません。</p>
      )}
      {storage.error && <p role="alert" className="break-words text-amber-200">{storage.error}</p>}
      <p className="text-xs text-neutral-400">設定ファイルにはハッシュのみを保存します。画面を閉じると、表示中のキーは画面の状態から破棄します。</p>
    </div>
  );
}
