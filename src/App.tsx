import { useCallback, useEffect, useState } from "react";
import { deleteKey, listKeys, setKey } from "./lib/api";
import type { KeyInfo } from "./lib/types";

const emptyForm = {
  service: "",
  name: "default",
  value: "",
  envVarName: "",
  expiresAt: "",
  note: "",
};

function App() {
  const [keys, setKeys] = useState<KeyInfo[]>([]);
  const [form, setForm] = useState(emptyForm);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setKeys(await listKeys());
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setMessage(null);
    try {
      const result = await setKey({
        service: form.service,
        name: form.name,
        value: form.value,
        envVarName: form.envVarName,
        expiresAt: form.expiresAt || undefined,
        note: form.note || undefined,
      });
      setMessage(
        `${result.service}/${result.name} を${result.action === "created" ? "登録" : "更新"}しました(${result.value_masked})`,
      );
      setForm(emptyForm);
      setShowForm(false);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleDelete(key: KeyInfo) {
    if (!window.confirm(`${key.service}/${key.name} を削除しますか?`)) return;
    setError(null);
    setMessage(null);
    try {
      const deleted = await deleteKey(key.service, key.name);
      setMessage(`${deleted} を削除しました`);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  const field =
    "w-full rounded-md border border-slate-300 px-3 py-2 text-sm focus:border-indigo-500 focus:outline-none";

  return (
    <main className="mx-auto min-h-screen max-w-4xl bg-slate-50 px-6 py-8 text-slate-900">
      <header className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">KeyNest</h1>
          <p className="text-sm text-slate-500">
            APIキーはOSのセキュアストレージに保存されます。表示は常にマスク値のみです。
          </p>
        </div>
        <button
          onClick={() => setShowForm((v) => !v)}
          className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700"
        >
          {showForm ? "閉じる" : "+ キーを追加"}
        </button>
      </header>

      {message && (
        <div className="mb-4 rounded-md bg-emerald-50 px-4 py-2 text-sm text-emerald-800">
          {message}
        </div>
      )}
      {error && (
        <div className="mb-4 rounded-md bg-red-50 px-4 py-2 text-sm text-red-800">{error}</div>
      )}

      {showForm && (
        <form
          onSubmit={handleSubmit}
          className="mb-6 grid grid-cols-2 gap-4 rounded-lg border border-slate-200 bg-white p-4"
        >
          <label className="text-sm">
            サービス *
            <input
              required
              className={field}
              placeholder="anthropic"
              value={form.service}
              onChange={(e) => setForm({ ...form, service: e.target.value })}
            />
          </label>
          <label className="text-sm">
            キー名 *
            <input
              required
              className={field}
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
              className={field}
              placeholder="sk-..."
              value={form.value}
              onChange={(e) => setForm({ ...form, value: e.target.value })}
            />
          </label>
          <label className="text-sm">
            環境変数名 *
            <input
              required
              className={field}
              placeholder="ANTHROPIC_API_KEY"
              value={form.envVarName}
              onChange={(e) => setForm({ ...form, envVarName: e.target.value })}
            />
          </label>
          <label className="text-sm">
            有効期限
            <input
              type="date"
              className={field}
              value={form.expiresAt}
              onChange={(e) => setForm({ ...form, expiresAt: e.target.value })}
            />
          </label>
          <label className="col-span-2 text-sm">
            メモ
            <input
              className={field}
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
              <th className="px-4 py-2">有効期限</th>
              <th className="px-4 py-2">メモ</th>
              <th className="px-4 py-2"></th>
            </tr>
          </thead>
          <tbody>
            {keys.map((key) => (
              <tr key={key.key_id} className="border-t border-slate-100">
                <td className="px-4 py-2 font-medium">{key.service}</td>
                <td className="px-4 py-2">{key.name}</td>
                <td className="px-4 py-2 font-mono text-xs">{key.env_var_name}</td>
                <td className="px-4 py-2">{key.expires_at ?? "—"}</td>
                <td className="px-4 py-2 text-slate-500">{key.note ?? ""}</td>
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
    </main>
  );
}

export default App;
