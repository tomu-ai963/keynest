import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { assignKey, deleteProject, inject, unassignKey } from "../lib/api";
import type { KeyInfo, ProjectInfo, ProjectKey } from "../lib/types";
import { fieldClass } from "./KeysView";

interface Props {
  projects: ProjectInfo[];
  keys: KeyInfo[];
  onChanged: () => Promise<void>;
  notify: (message: string | null, error: string | null) => void;
}

function ProjectsView({ projects, keys, onChanged, notify }: Props) {
  const [newName, setNewName] = useState("");
  const [newPath, setNewPath] = useState("");
  // プロジェクトごとの「追加するキー」選択値(key: project name, value: "service/name")
  const [selection, setSelection] = useState<Record<string, string>>({});

  async function pickFolder(onPicked: (path: string) => void) {
    try {
      const picked = await open({ directory: true });
      if (typeof picked === "string") onPicked(picked);
    } catch (e) {
      notify(null, String(e));
    }
  }

  async function handleCreate(e: React.FormEvent) {
    e.preventDefault();
    const first = keys[0];
    if (!first) {
      notify(null, "プロジェクトを作るには、先にキーを1件以上登録してください");
      return;
    }
    // プロジェクト作成 = 最初のキーの紐づけ(vault の設計に合わせる)
    try {
      await assignKey(first.service, first.name, newName, newPath || undefined);
      notify(`プロジェクト ${newName} を作成し、${first.service}/${first.name} を紐づけました`, null);
      setNewName("");
      setNewPath("");
      await onChanged();
    } catch (e) {
      notify(null, String(e));
    }
  }

  async function handleAssign(project: ProjectInfo) {
    const value = selection[project.name];
    if (!value) return;
    const [service, ...rest] = value.split("/");
    try {
      const result = await assignKey(service, rest.join("/"), project.name);
      notify(`${result.key} を ${project.name} に紐づけました`, null);
      setSelection({ ...selection, [project.name]: "" });
      await onChanged();
    } catch (e) {
      notify(null, String(e));
    }
  }

  async function handleUnassign(project: ProjectInfo, key: ProjectKey) {
    try {
      await unassignKey(key.service, key.name, project.name);
      notify(`${key.service}/${key.name} の紐づけを解除しました`, null);
      await onChanged();
    } catch (e) {
      notify(null, String(e));
    }
  }

  async function handleInject(project: ProjectInfo) {
    const target = project.path ?? "(未設定)";
    if (
      !window.confirm(
        `${project.name} のキーを次の場所の .env に書き出します:\n${target}\n\n` +
          "APIキーの生の値がファイルに書き込まれます。よろしいですか?",
      )
    ) {
      return;
    }
    try {
      const result = await inject(project.name);
      notify(`${result.env_path} に書き出しました: ${result.written.join(", ")}`, null);
      await onChanged();
    } catch (e) {
      notify(null, String(e));
    }
  }

  async function handleSetPath(project: ProjectInfo) {
    await pickFolder(async (path) => {
      const key = project.keys[0];
      if (!key) return;
      try {
        // パス更新は assign の path 上書きを利用
        await assignKey(key.service, key.name, project.name, path);
        notify(`${project.name} のパスを設定しました: ${path}`, null);
        await onChanged();
      } catch (e) {
        notify(null, String(e));
      }
    });
  }

  async function handleDelete(project: ProjectInfo) {
    if (!window.confirm(`プロジェクト ${project.name} を削除しますか?(キー本体は残ります)`)) return;
    try {
      await deleteProject(project.name);
      notify(`プロジェクト ${project.name} を削除しました`, null);
      await onChanged();
    } catch (e) {
      notify(null, String(e));
    }
  }

  const assignable = (project: ProjectInfo) =>
    keys.filter((k) => !project.keys.some((pk) => pk.key_id === k.key_id));

  return (
    <section className="space-y-6">
      <form
        onSubmit={handleCreate}
        className="flex items-end gap-3 rounded-lg border border-slate-200 bg-white p-4"
      >
        <label className="flex-1 text-sm">
          新しいプロジェクト名 *
          <input
            required
            className={fieldClass}
            placeholder="my-app"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
          />
        </label>
        <label className="flex-1 text-sm">
          パス(.env の書き出し先)
          <span className="flex gap-2">
            <input
              className={fieldClass}
              placeholder="C:\projects\my-app"
              value={newPath}
              onChange={(e) => setNewPath(e.target.value)}
            />
            <button
              type="button"
              onClick={() => pickFolder(setNewPath)}
              className="whitespace-nowrap rounded-md border border-slate-300 px-3 py-2 text-sm hover:bg-slate-100"
            >
              選択...
            </button>
          </span>
        </label>
        <button
          type="submit"
          className="rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700"
        >
          作成
        </button>
      </form>

      {projects.length === 0 ? (
        <p className="rounded-lg border border-dashed border-slate-300 p-8 text-center text-sm text-slate-500">
          プロジェクトはまだありません。作成すると最初のキーが自動で紐づきます。
        </p>
      ) : (
        projects.map((project) => (
          <div key={project.project_id} className="rounded-lg border border-slate-200 bg-white p-4">
            <div className="mb-3 flex items-center justify-between">
              <div>
                <h2 className="font-semibold">{project.name}</h2>
                <p className="font-mono text-xs text-slate-500">
                  {project.path ?? "パス未設定"}
                  <button
                    onClick={() => handleSetPath(project)}
                    className="ml-2 text-indigo-500 underline"
                  >
                    変更
                  </button>
                </p>
              </div>
              <div className="flex gap-2">
                <button
                  onClick={() => handleInject(project)}
                  className="rounded-md bg-emerald-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-emerald-700"
                >
                  .env に書き出し
                </button>
                <button
                  onClick={() => handleDelete(project)}
                  className="rounded-md px-2 py-1 text-xs text-red-600 hover:bg-red-50"
                >
                  削除
                </button>
              </div>
            </div>

            <ul className="mb-3 divide-y divide-slate-100 text-sm">
              {project.keys.map((key) => (
                <li key={key.key_id} className="flex items-center justify-between py-1.5">
                  <span>
                    <span className="font-medium">{key.service}</span>/{key.name}
                    <span className="ml-2 font-mono text-xs text-slate-500">
                      {key.env_var_name}
                    </span>
                  </span>
                  <button
                    onClick={() => handleUnassign(project, key)}
                    className="text-xs text-slate-400 hover:text-red-600"
                  >
                    解除
                  </button>
                </li>
              ))}
            </ul>

            {assignable(project).length > 0 && (
              <div className="flex gap-2">
                <select
                  className={fieldClass}
                  value={selection[project.name] ?? ""}
                  onChange={(e) => setSelection({ ...selection, [project.name]: e.target.value })}
                >
                  <option value="">キーを追加...</option>
                  {assignable(project).map((k) => (
                    <option key={k.key_id} value={`${k.service}/${k.name}`}>
                      {k.service}/{k.name}({k.env_var_name})
                    </option>
                  ))}
                </select>
                <button
                  onClick={() => handleAssign(project)}
                  className="whitespace-nowrap rounded-md border border-slate-300 px-3 py-2 text-sm hover:bg-slate-100"
                >
                  紐づけ
                </button>
              </div>
            )}
          </div>
        ))
      )}
    </section>
  );
}

export default ProjectsView;
