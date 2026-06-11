import { useCallback, useEffect, useState } from "react";
import { listKeys, listProjects } from "./lib/api";
import type { KeyInfo, ProjectInfo } from "./lib/types";
import KeysView from "./views/KeysView";
import ProjectsView from "./views/ProjectsView";

type Tab = "keys" | "projects";

function App() {
  const [tab, setTab] = useState<Tab>("keys");
  const [keys, setKeys] = useState<KeyInfo[]>([]);
  const [projects, setProjects] = useState<ProjectInfo[]>([]);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const notify = useCallback((msg: string | null, err: string | null) => {
    setMessage(msg);
    setError(err);
  }, []);

  const refresh = useCallback(async () => {
    try {
      const [k, p] = await Promise.all([listKeys(), listProjects()]);
      setKeys(k);
      setProjects(p);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const tabClass = (t: Tab) =>
    `rounded-md px-4 py-2 text-sm font-medium ${
      tab === t ? "bg-indigo-600 text-white" : "text-slate-600 hover:bg-slate-200"
    }`;

  return (
    <main className="mx-auto min-h-screen max-w-4xl bg-slate-50 px-6 py-8 text-slate-900">
      <header className="mb-6">
        <h1 className="text-2xl font-bold">KeyNest</h1>
        <p className="text-sm text-slate-500">
          APIキーはOSのセキュアストレージに保存されます。表示は常にマスク値のみです。
        </p>
      </header>

      <nav className="mb-6 flex gap-2">
        <button className={tabClass("keys")} onClick={() => setTab("keys")}>
          キー({keys.length})
        </button>
        <button className={tabClass("projects")} onClick={() => setTab("projects")}>
          プロジェクト({projects.length})
        </button>
      </nav>

      {message && (
        <div className="mb-4 rounded-md bg-emerald-50 px-4 py-2 text-sm text-emerald-800">
          {message}
        </div>
      )}
      {error && (
        <div className="mb-4 rounded-md bg-red-50 px-4 py-2 text-sm text-red-800">{error}</div>
      )}

      {tab === "keys" ? (
        <KeysView keys={keys} onChanged={refresh} notify={notify} />
      ) : (
        <ProjectsView projects={projects} keys={keys} onChanged={refresh} notify={notify} />
      )}
    </main>
  );
}

export default App;
