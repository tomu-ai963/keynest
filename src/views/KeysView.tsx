import { Fragment, useEffect, useState } from "react";
import {
  deleteKey,
  listPresets,
  listRotations,
  rotateKey,
  setKey,
  validateKey,
} from "../lib/api";
import type { KeyInfo, Preset, RotationInfo } from "../lib/types";

const emptyForm = {
  preset: "",
  service: "",
  name: "default",
  value: "",
  envVarName: "",
  expiresAt: "",
  note: "",
};

interface Props {
  keys: KeyInfo[];
  onChanged: () => Promise<void>;
  notify: (message: string | null, error: string | null) => void;
}

export const fieldClass =
  "w-full rounded-md border border-slate-300 px-3 py-2 text-sm focus:border-indigo-500 focus:outline-none";

// 有効期限までの日数(負なら期限切れ)。expires_at は "YYYY-MM-DD" 形式。
function daysUntil(expiresAt: string): number {
  const expiry = new Date(expiresAt.slice(0, 10) + "T00:00:00");
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  return Math.round((expiry.getTime() - today.getTime()) / 86400000);
}

const EXPIRY_WARN_DAYS = 30;

function KeysView({ keys, onChanged, notify }: Props) {
  const [presets, setPresets] = useState<Preset[]>([]);
  const [form, setForm] = useState(emptyForm);
  const [showForm, setShowForm] = useState(false);
  const [validating, setValidating] = useState<number | null>(null);
  // ローテーションパネルを開いているキー
  const [rotating, setRotating] = useState<number | null>(null);
  const [rotateValue, setRotateValue] = useState("");
  const [history, setHistory] = useState<RotationInfo[]>([]);

  useEffect(() => {
    listPresets().then(setPresets).catch((e) => notify(null, String(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  function applyPreset(id: string) {
    const preset = presets.find((p) => p.id === id);
    setForm({
      ...form,
      preset: id,
      service: preset ? preset.id : form.service,
      envVarName: preset ? preset.env_var : form.envVarName,
    });
  }

  const selectedPreset = presets.find((p) => p.id === form.preset);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    try {
      const result = await setKey({
        service: form.service,
        name: form.name,
        value: form.value,
        envVarName: form.envVarName,
        expiresAt: form.expiresAt || undefined,
        note: form.note || undefined,
      });
      notify(
        `${result.service}/${result.name} を${result.action === "created" ? "登録" : "更新"}しました(${result.value_masked})`,
        null,
      );
      setForm(emptyForm);
      setShowForm(false);
      await onChanged();
    } catch (e) {
      notify(null, String(e));
    }
  }

  async function handleValidate(key: KeyInfo) {
    setValidating(key.key_id);
    try {
      const result = await validateKey(key.service, key.name);
      if (result.ok) {
        notify(`${result.key} は有効です(${result.detail})`, null);
      } else {
        notify(null, `${result.key} の検証に失敗しました(${result.detail})`);
      }
      await onChanged();
    } catch (e) {
      notify(null, String(e));
    } finally {
      setValidating(null);
    }
  }

  async function toggleRotate(key: KeyInfo) {
    if (rotating === key.key_id) {
      setRotating(null);
      return;
    }
    setRotating(key.key_id);
    setRotateValue("");
    try {
      setHistory(await listRotations(key.service, key.name));
    } catch {
      setHistory([]);
    }
  }

  async function handleRotate(e: React.FormEvent, key: KeyInfo) {
    e.preventDefault();
    try {
      const result = await rotateKey(key.service, key.name, rotateValue);
      notify(
        `${result.key} をローテーションしました(${result.old_value_masked ?? "—"} → ${result.value_masked})`,
        null,
      );
      setRotating(null);
      setRotateValue("");
      await onChanged();
    } catch (e) {
      notify(null, String(e));
    }
  }

  async function handleDelete(key: KeyInfo) {
    if (!window.confirm(`${key.service}/${key.name} を削除しますか?`)) return;
    try {
      const deleted = await deleteKey(key.service, key.name);
      notify(`${deleted} を削除しました`, null);
      await onChanged();
    } catch (e) {
      notify(null, String(e));
    }
  }

  const expiryAlerts = keys
    .filter((k) => k.expires_at !== null)
    .map((k) => ({ key: k, days: daysUntil(k.expires_at as string) }))
    .filter(({ days }) => days <= EXPIRY_WARN_DAYS)
    .sort((a, b) => a.days - b.days);

  return (
    <section>
      {expiryAlerts.length > 0 && (
        <div className="mb-4 rounded-md border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-900">
          <p className="mb-1 font-medium">有効期限アラート</p>
          <ul className="list-inside list-disc">
            {expiryAlerts.map(({ key, days }) => (
              <li key={key.key_id}>
                {key.service}/{key.name} —{" "}
                {days < 0
                  ? `${-days}日前に期限切れ`
                  : days === 0
                    ? "今日が期限"
                    : `あと${days}日(${key.expires_at})`}
              </li>
            ))}
          </ul>
        </div>
      )}

      <div className="mb-4 flex justify-end">
        <button
          onClick={() => setShowForm((v) => !v)}
          className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700"
        >
          {showForm ? "閉じる" : "+ キーを追加"}
        </button>
      </div>

      {showForm && (
        <form
          onSubmit={handleSubmit}
          className="mb-6 grid grid-cols-2 gap-4 rounded-lg border border-slate-200 bg-white p-4"
        >
          <label className="col-span-2 text-sm">
            プリセット
            <select
              className={fieldClass}
              value={form.preset}
              onChange={(e) => applyPreset(e.target.value)}
            >
              <option value="">カスタム(手入力)</option>
              {presets.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.display_name}
                </option>
              ))}
            </select>
          </label>
          <label className="text-sm">
            サービス *
            <input
              required
              className={fieldClass}
              placeholder="anthropic"
              value={form.service}
              onChange={(e) => setForm({ ...form, service: e.target.value })}
            />
          </label>
          <label className="text-sm">
            キー名 *
            <input
              required
              className={fieldClass}
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
            />
          </label>
          <label className="col-span-2 text-sm">
            値 *
            <input
              required
              type="password"
              autoComplete="off"
              className={fieldClass}
              placeholder="sk-..."
              value={form.value}
              onChange={(e) => setForm({ ...form, value: e.target.value })}
            />
            {selectedPreset && (
              <span className="text-xs text-slate-400">
                キーの発行:{" "}
                <a
                  href={selectedPreset.key_url}
                  target="_blank"
                  rel="noreferrer"
                  className="text-indigo-500 underline"
                >
                  {selectedPreset.key_url}
                </a>
              </span>
            )}
          </label>
          <label className="text-sm">
            環境変数名 *
            <input
              required
              className={fieldClass}
              placeholder="ANTHROPIC_API_KEY"
              value={form.envVarName}
              onChange={(e) => setForm({ ...form, envVarName: e.target.value })}
            />
          </label>
          <label className="text-sm">
            有効期限
            <input
              type="date"
              className={fieldClass}
              value={form.expiresAt}
              onChange={(e) => setForm({ ...form, expiresAt: e.target.value })}
            />
          </label>
          <label className="col-span-2 text-sm">
            メモ
            <input
              className={fieldClass}
              value={form.note}
              onChange={(e) => setForm({ ...form, note: e.target.value })}
            />
          </label>
          <div className="col-span-2">
            <button
              type="submit"
              className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700"
            >
              保存
            </button>
          </div>
        </form>
      )}

      {keys.length === 0 ? (
        <p className="rounded-lg border border-dashed border-slate-300 p-8 text-center text-sm text-slate-500">
          まだキーが登録されていません。「+ キーを追加」から登録してください。
        </p>
      ) : (
        <table className="w-full overflow-hidden rounded-lg border border-slate-200 bg-white text-sm">
          <thead className="bg-slate-100 text-left text-xs uppercase text-slate-500">
            <tr>
              <th className="px-4 py-2">サービス</th>
              <th className="px-4 py-2">キー名</th>
              <th className="px-4 py-2">環境変数名</th>
              <th className="px-4 py-2">使用プロジェクト</th>
              <th className="px-4 py-2">有効期限</th>
              <th className="px-4 py-2">最終検証</th>
              <th className="px-4 py-2"></th>
            </tr>
          </thead>
          <tbody>
            {keys.map((key) => {
              const days = key.expires_at === null ? null : daysUntil(key.expires_at);
              return (
                <Fragment key={key.key_id}>
                  <tr className="border-t border-slate-100">
                    <td className="px-4 py-2 font-medium">{key.service}</td>
                    <td className="px-4 py-2">{key.name}</td>
                    <td className="px-4 py-2 font-mono text-xs">{key.env_var_name}</td>
                    <td className="px-4 py-2">
                      {key.used_by.map((p) => (
                        <span
                          key={p}
                          className="mr-1 rounded-full bg-indigo-50 px-2 py-0.5 text-xs text-indigo-700"
                        >
                          {p}
                        </span>
                      ))}
                    </td>
                    <td className="px-4 py-2">
                      {key.expires_at === null ? (
                        "—"
                      ) : days !== null && days < 0 ? (
                        <span className="rounded bg-red-100 px-1.5 py-0.5 text-xs font-medium text-red-700">
                          期限切れ
                        </span>
                      ) : days !== null && days <= EXPIRY_WARN_DAYS ? (
                        <span className="rounded bg-amber-100 px-1.5 py-0.5 text-xs font-medium text-amber-800">
                          あと{days}日
                        </span>
                      ) : (
                        key.expires_at
                      )}
                    </td>
                    <td className="px-4 py-2 text-xs text-slate-500">
                      {key.last_validated_at ? key.last_validated_at.slice(0, 10) : "未検証"}
                    </td>
                    <td className="whitespace-nowrap px-4 py-2 text-right">
                      {key.validatable && (
                        <button
                          onClick={() => handleValidate(key)}
                          disabled={validating === key.key_id}
                          className="rounded-md px-2 py-1 text-xs text-indigo-600 hover:bg-indigo-50 disabled:opacity-50"
                        >
                          {validating === key.key_id ? "検証中..." : "検証"}
                        </button>
                      )}
                      <button
                        onClick={() => toggleRotate(key)}
                        className="rounded-md px-2 py-1 text-xs text-slate-600 hover:bg-slate-100"
                      >
                        ローテーション
                      </button>
                      <button
                        onClick={() => handleDelete(key)}
                        className="rounded-md px-2 py-1 text-xs text-red-600 hover:bg-red-50"
                      >
                        削除
                      </button>
                    </td>
                  </tr>
                  {rotating === key.key_id && (
                    <tr className="border-t border-slate-100 bg-slate-50">
                      <td colSpan={7} className="px-4 py-3">
                        <form
                          onSubmit={(e) => handleRotate(e, key)}
                          className="flex items-end gap-2"
                        >
                          <label className="flex-1 text-sm">
                            新しい値 *
                            <input
                              required
                              type="password"
                              autoComplete="off"
                              className={fieldClass}
                              placeholder="sk-..."
                              value={rotateValue}
                              onChange={(e) => setRotateValue(e.target.value)}
                            />
                          </label>
                          <button
                            type="submit"
                            className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700"
                          >
                            ローテーション実行
                          </button>
                        </form>
                        {history.length > 0 && (
                          <div className="mt-3 text-xs text-slate-500">
                            <p className="mb-1 font-medium">ローテーション履歴</p>
                            <ul className="space-y-0.5">
                              {history.map((r, i) => (
                                <li key={i}>
                                  {r.rotated_at} — 旧値:{" "}
                                  <span className="font-mono">{r.old_value_masked || "—"}</span>
                                </li>
                              ))}
                            </ul>
                          </div>
                        )}
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })}
          </tbody>
        </table>
      )}
    </section>
  );
}

export default KeysView;
