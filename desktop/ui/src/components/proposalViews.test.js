import { beforeEach, describe, expect, it } from "bun:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { invoke } from "../test/tauriMock";
import { manualCases, proposal } from "../test/proposalFixtures";
import { GaiaError } from "../api";
import { MANUAL_FORMS } from "../forms/manualFields";
import { buildManualIntent } from "../forms/manualPayload";
import { ManualSave } from "../lib/manualSave";
import { ProposalDecisions } from "../lib/proposalDecisions";
import AddForms from "./AddForms";
import Detail from "./Detail";
import ManualField from "./ManualField";
import ManualSaveStatus from "./ManualSaveStatus";
import ProposalCard from "./ProposalCard";
import ProposalFeedback from "./ProposalFeedback";
import Proposals, { ProposalCount } from "./Proposals";
import SaveNotice from "./SaveNotice";

const noop = () => {};
const render = (component, props) => renderToStaticMarkup(createElement(component, props));
const baseInput = { target_type: "person", action: "insert", patch: { name: "秘密の人物" }, kind: "fact", scope: "scope-a", request_id: "fixed-request" };
const failed = { input: baseInput, phase: "error", proposalId: 31, proposalStatus: "pending", error: new GaiaError({ code: "busy", message: "retry later", details: { proposal_id: 31 } }) };
const fakeController = (snapshot) => ({ getSnapshot: () => snapshot, subscribe: () => noop, retry: noop, checkStatus: noop });

beforeEach(() => invoke.mockReset());

describe("proposal queue presentation", () => {
  it("shows the exact patch, client, request ID, kind and provenance", () => {
    const full = { ...proposal, kind: "inference", target_id: 3, provenance: { system: "minutes", uri: "custom://record/3", note: "元記録の文脈", snapshot: "記録の要点" }, provenance_id: 12 };
    const html = render(ProposalCard, { proposal: full, onDecide: noop, openDetail: noop });
    for (const text of ["提案 #31", "scope-a", "demo-agent", "request-0001", "inference（推測）", "提案の人物", "2026-08-27T10:00:00Z", "参照 ID: 12", "custom://record/3", "元記録の文脈", "記録の要点", "承認する", "却下する"]) expect(html).toContain(text);
    expect(html).toContain('for="reason-31"');
    expect(html).not.toContain("href=");
  });

  it("does not expose mutation buttons for approved or rejected proposals", () => {
    for (const status of ["approved", "rejected"]) {
      const html = render(ProposalCard, { proposal: { ...proposal, status, result_id: 7, decided_by: "human", decided_at: "2026-08-28", decision_note: "判断の理由" }, onDecide: noop, openDetail: noop });
      expect(html).not.toContain("承認する");
      expect(html).not.toContain("却下する");
      expect(html).not.toContain("textarea");
      expect(html).toContain("判断の理由");
      expect(html).toContain("human");
      expect(html).toContain("person #7");
    }
  });

  it("disables pending decisions and retains structured errors with a retry action", () => {
    const operation = { proposalId: 31, scope: "scope-a", decision: "approve", reason: "", busy: true, error: null };
    const loading = render(ProposalCard, { proposal, operation, onDecide: noop, openDetail: noop });
    expect(loading).toContain('aria-busy="true"');
    expect(loading).toContain('type="button" disabled=""');
    const html = render(ProposalCard, { proposal, operation: { ...operation, busy: false, error: failed.error }, onDecide: noop, openDetail: noop });
    expect(html).toContain("busy: retry later");
    expect(html).toContain("proposal_id");
    expect(html).toContain("同じ承認操作を再試行");
  });

  it("escapes executable-looking patch and provenance content as plain text", () => {
    const html = render(ProposalCard, { proposal: { ...proposal, patch: { name: "<script>run()</script>" }, provenance: { uri: "javascript:run()", note: "<img src=x>" } }, onDecide: noop, openDetail: noop });
    expect(html).toContain("&lt;script&gt;");
    expect(html).toContain("&lt;img src=x&gt;");
    expect(html).not.toContain("<script>");
    expect(html).not.toContain("<img ");
    expect(html).not.toContain("href=");
  });

  it("distinguishes first loading, empty results and retrieval limits", () => {
    const html = render(Proposals, { scope: "scope-a", decisions: new ProposalDecisions(), openDetail: noop });
    expect(html).toContain("提案を読み込んでいます");
    expect(html).not.toContain("提案はありません");
    expect(html).toContain("新しい順に最大 50 件");
    expect(html).toContain("ページ送りはありません");
    expect(render(ProposalCount, { count: 0, status: "pending", limit: 50 })).toContain("未承認の提案はありません");
    expect(render(ProposalCount, { count: 0, status: "rejected", limit: 50 })).toContain("却下済みの提案はありません");
    expect(render(ProposalCount, { count: 50, status: "pending", limit: 50 })).toContain("ほかにも該当する提案がある可能性");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("does not request a queue before its scope is known", () => {
    const html = render(Proposals, { scope: "", decisions: new ProposalDecisions(), openDetail: noop });
    expect(html).toContain("scope を入力するか");
    expect(html).toContain('type="button" disabled=""');
    expect(html).not.toContain("提案を読み込んでいます");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("keeps a successful decision visible after it disappears from the filtered list", () => {
    const operation = { proposalId: 31, scope: "scope-a", busy: false, error: null, decision: "approve", reason: "", output: { proposal_id: 31, status: "approved", result: { target_type: "person", id: 7 } } };
    expect(render(ProposalFeedback, { operations: [operation], visibleIds: [], openDetail: noop })).toContain("承認しました");
    expect(render(ProposalFeedback, { operations: [operation], visibleIds: [31], openDetail: noop })).toBe("");
  });
});

describe("manual form and operation presentation", () => {
  it("labels every field, marks optional values and keeps URI fields as text", () => {
    for (const form of Object.values(MANUAL_FORMS)) {
      for (const field of form.fields) {
        const html = render(ManualField, { field, value: "", disabled: false, onChange: noop });
        expect(html).toContain(`for="manual-${field.key}"`);
        expect(html).toContain(field.required ? "（必須）" : "（任意）");
        if (field.required) expect(html).toContain('required=""');
        if (field.key === "uri") expect(html).toContain('type="text"');
      }
    }
  });

  it("starts with no save operation and does not send anything during rendering", () => {
    const html = render(AddForms, { scope: "scope-a", controller: new ManualSave(), openDetail: noop, restoreOperation: noop, showProposals: noop });
    expect(html).toContain("提案・承認して保存");
    expect(html).toContain("任意項目は空欄なら送信しません");
    expect(html).toContain("human クライアント");
    expect(html).not.toContain("保存済みです");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("restores all seven saved payloads without starting another save", async () => {
    for (const sample of manualCases) {
      const controller = new ManualSave({ propose: async () => ({ proposal_id: 31, status: "pending", duplicate: false }), approve: async () => { throw new Error("retry"); }, findFinalized: async () => null });
      await controller.start(buildManualIntent(sample.target, sample.values, sample.kind ?? "", "scope-a"));
      const html = render(AddForms, { scope: "scope-a", controller, openDetail: noop, restoreOperation: noop, showProposals: noop });
      expect(html).toContain(`<option value="${sample.target}" selected="">`);
      expect(html).toContain("承認だけ再試行");
      expect(html).toContain('fieldset disabled=""');
    }
    expect(invoke).not.toHaveBeenCalled();
  });

  it("keeps the proposal ID and original scope visible after approval failure", () => {
    const html = render(ManualSaveStatus, { operation: failed, controller: fakeController(failed), openDetail: noop, showProposals: noop, onClear: noop });
    for (const text of ["scope-a", "fixed-request", "31", "承認だけ再試行", "承認の結果を確認するまで入力を固定", "タブを切り替えてもこの操作を保持", "アプリ終了時には途中状態が失われます"]) expect(html).toContain(text);
    expect(html).not.toContain("新しい入力へ");
    expect(html).not.toContain("入力を修正する");
  });

  it("never displays the previous scope's payload in the new scope's form", () => {
    const controller = fakeController(failed);
    const html = render(AddForms, { scope: "scope-b", controller, openDetail: noop, restoreOperation: noop, showProposals: noop });
    expect(html).toContain("別の scope の保存操作を保持");
    expect(html).toContain("元の scope の保存操作を確認");
    expect(html).not.toContain("秘密の人物");
    expect(html).not.toContain("fixed-request");
    expect(html).not.toContain("提案・承認して保存");
    const notice = render(SaveNotice, { controller, visibleInForm: false, onRestore: noop });
    expect(notice).toContain("未完了の手入力を保持");
    expect(notice).not.toContain("秘密の人物");
    expect(notice).not.toContain("scope-a");
  });

  it("returns from an entity detail to the correct originating feature", () => {
    const html = render(Detail, { target: { type: "person", id: 7 }, scope: "scope-a", onBack: noop, openDetail: noop, backLabel: "提案へ戻る" });
    expect(html).toContain("提案へ戻る");
    expect(html).not.toContain("検索へ戻る");
  });
});
