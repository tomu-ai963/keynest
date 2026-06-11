import { useEffect, useState } from "react";
import { deleteKey, listPresets, setKey } from "../lib/api";
import type { KeyInfo, Preset } from "../lib/types";

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

function KeysView({ keys, onChanged, notify }: Props) {
  const [presets, setPresets] = useState<Preset[]>([]);
  const [form, setForm] = useState(emptyForm);
  const [showForm, setShowForm] = useState(false);

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

  return (
    <section>
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
              <th className="px-4 py-2"></th>
            </tr>
          </thead>
          <tbody>
            {keys.map((key) => (
              <tr key={key.key_id} className="border-t border-slate-100">
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
                <td className="px-4 py-2">{key.expires_at ?? "—"}</td>
                <td className="px-4 py-2 text-right">
                  <button
                    onClick={() => handleDelete(key)}
                    className="rounded-md px-2 py-1 text-xs text-red-600 hover:bg-red-50"
                  >
                    削除
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </section>
  );
}

export default KeysView;
