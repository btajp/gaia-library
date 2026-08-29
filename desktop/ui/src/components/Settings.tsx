import AffiliationsSettings from "./settings/AffiliationsSettings";
import ClientsSettings from "./settings/ClientsSettings";
import CliSettings from "./settings/CliSettings";
import { ServerSettings, VersionSettings } from "./settings/RuntimeSettings";

export default function Settings() {
  return (
    <div className="space-y-6">
      <div>
        <h2 className="text-xl font-semibold">設定</h2>
        <p className="mt-2 text-sm leading-6 text-neutral-400">接続元（クライアント）と機密境界（所属元 = scope）の管理です。エージェント用のキー発行、接続設定の表示、名前の変更、CLI リンクを行います。</p>
        <p className="mt-1 text-sm leading-6 text-neutral-400">所属元とクライアントの設定は、上の対象 scope の選択に関係なくアプリ全体に適用されます。</p>
      </div>
      <AffiliationsSettings />
      <ClientsSettings />
      <ServerSettings />
      <CliSettings />
      <VersionSettings />
    </div>
  );
}
