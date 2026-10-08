import { useCallback, useEffect, useRef, useState } from "react";
import {
  deleteAdapter,
  listAdapters,
  reloadAdapters,
  saveAdapter,
  setAdapterEnabled,
} from "../services/desktop";
import type { AdapterInput, AdapterKind, AdaptersView, AdapterView } from "../types/desktop";
import { logError } from "../utils/logger";

/** How often the window re-reads status. Slow: this is a settings surface. */
const POLL_INTERVAL_MS = 2000;
/** How long a success/error banner stays up. */
const BANNER_TIMEOUT_MS = 4000;

/**
 * The Adapter Management window.
 *
 * One row per configured adapter - name, type, source, enabled and last status -
 * and a small form to add or edit one. It is deliberately not a dashboard: there
 * are no logs, no task lists and no per-task detail, because the Harness Task
 * Note already answers "what is running".
 *
 * All validation lives in Rust. This component never decides whether a
 * configuration is legal; it sends the edit and renders whatever comes back, so
 * the file on disk and the rules that guard it stay in one place.
 */
export function AdaptersWindow() {
  const [adapters, setAdapters] = useState<AdapterView[]>([]);
  const [running, setRunning] = useState(0);
  const [loaded, setLoaded] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  // Why the file on disk is unusable, if it is. Kept apart from `problem`
  // because it is a standing state of the file, not the result of an action, so
  // it must not be cleared by the banner timeout.
  const [fileProblem, setFileProblem] = useState<string | null>(null);
  const [editing, setEditing] = useState<AdapterInput | null>(null);
  const [busy, setBusy] = useState(false);

  const pollingRef = useRef(false);
  const editingNameRef = useRef<string | null>(null);

  const refresh = useCallback(async () => {
    if (pollingRef.current) return;
    pollingRef.current = true;
    try {
      const next = await listAdapters();
      setAdapters(next.adapters);
      setRunning(next.running);
      setFileProblem(next.problem ?? null);
      setLoaded(true);
    } catch (cause) {
      logError("could not read the adapters", cause);
      setProblem("适配器加载失败。");
    } finally {
      pollingRef.current = false;
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), POLL_INTERVAL_MS);
    return () => window.clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    if (!notice && !problem) return;
    const timer = window.setTimeout(() => {
      setNotice(null);
      setProblem(null);
    }, BANNER_TIMEOUT_MS);
    return () => window.clearTimeout(timer);
  }, [notice, problem]);

  /**
   * Run one mutation.
   *
   * A rejected edit is a user-facing message, not a crash: Rust refused it and
   * the file is unchanged, so the list is simply re-read to show the truth.
   */
  const mutate = useCallback(
    async (label: string, action: () => Promise<AdaptersView>) => {
      setBusy(true);
      try {
        const next = await action();
        setAdapters(next.adapters);
        setRunning(next.running);
        setFileProblem(next.problem ?? null);
        setNotice(label);
        setProblem(null);
        return true;
      } catch (cause) {
        const reason = cause instanceof Error ? cause.message : String(cause);
        logError(`adapter action failed: ${label}`, cause);
        setProblem(reason);
        setNotice(null);
        return false;
      } finally {
        setBusy(false);
      }
    },
    [],
  );

  const startAdd = useCallback((kind: AdapterKind) => {
    editingNameRef.current = null;
    setEditing(
      kind === "local-json"
        ? { name: "", kind: "local-json", enabled: true, path: "" }
        : { name: "", kind: "local-http", enabled: true, port: 18001 },
    );
  }, []);

  const startEdit = useCallback((adapter: AdapterView) => {
    editingNameRef.current = adapter.name;
    setEditing({
      name: adapter.name,
      kind: adapter.kind,
      enabled: adapter.enabled,
      path: adapter.path ?? null,
      port: adapter.port ?? null,
      http_path: adapter.http_path ?? null,
      timeout_millis: adapter.timeout_millis ?? null,
      poll_interval_millis: adapter.poll_interval_millis ?? null,
    });
  }, []);

  const cancelEdit = useCallback(() => {
    editingNameRef.current = null;
    setEditing(null);
  }, []);

  const submit = useCallback(async () => {
    if (!editing) return;
    const previous = editingNameRef.current;
    const ok = await mutate(previous ? "适配器已保存" : "适配器已添加", () =>
      saveAdapter(editing, previous),
    );
    if (ok) cancelEdit();
  }, [cancelEdit, editing, mutate]);

  if (!loaded && adapters.length === 0) {
    return (
      <main className="adapters">
        <h1 className="adapters__title">适配器管理</h1>
        <p className="adapters__empty">正在加载适配器…</p>
      </main>
    );
  }

  return (
    <main className="adapters">
      <header className="adapters__header">
        <h1 className="adapters__title">适配器管理</h1>
        <p className="adapters__subtitle">
          {running === 0
            ? "当前没有运行中的适配器。"
            : `共 ${adapters.length} 个适配器，${running} 个运行中。`}
        </p>
      </header>

      {fileProblem && (
        <p className="adapters__problem">
          无法读取 harness-adapters.json：{fileProblem}
        </p>
      )}

      {(notice || problem) && (
        <p className={problem ? "adapters__problem" : "adapters__notice"}>
          {problem ?? notice}
        </p>
      )}

      {adapters.length === 0 ? (
        <p className="adapters__empty">
          {fileProblem
            ? "请修复上述文件，或在下方保存一个适配器以替换它。"
            : "尚未配置适配器。可在下方添加，或让本机 Harness 向本地端点上报。"}
        </p>
      ) : (
        <ul className="adapters__list">
          {adapters.map((adapter) => (
            <li key={adapter.name} className="adapters__row">
              <div className="adapters__main">
                <span className="adapters__name">{adapter.name}</span>
                <span className={`adapters__status adapters__status--${adapter.status}`}>
                  {adapter.status}
                </span>
                <span className="adapters__type">{adapter.kind}</span>
                <span className="adapters__source" title={adapter.source}>
                  {adapter.source || "—"}
                </span>
              </div>
              {adapter.detail && <p className="adapters__detail">{adapter.detail}</p>}
              <div className="adapters__actions">
                <label className="adapters__toggle">
                  <input
                    type="checkbox"
                    checked={adapter.enabled}
                    disabled={busy}
                    onChange={(event) =>
                      void mutate(
                        event.target.checked ? "适配器已启用" : "适配器已停用",
                        () => setAdapterEnabled(adapter.name, event.target.checked),
                      )
                    }
                  />
                  启用
                </label>
                <button type="button" disabled={busy} onClick={() => startEdit(adapter)}>
                  编辑
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() =>
                    void mutate("适配器已删除", () => deleteAdapter(adapter.name))
                  }
                >
                  Delete
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}

      {editing ? (
        <AdapterForm
          value={editing}
          busy={busy}
          onChange={setEditing}
          onSubmit={() => void submit()}
          onCancel={cancelEdit}
        />
      ) : (
        <div className="adapters__add">
          <button type="button" disabled={busy} onClick={() => startAdd("local-json")}>
            添加本地 JSON
          </button>
          <button type="button" disabled={busy} onClick={() => startAdd("local-http")}>
            添加本地 HTTP
          </button>
          <button type="button" disabled={busy} onClick={() => void mutate("适配器已重新加载", reloadAdapters)}>
            Reload
          </button>
        </div>
      )}
    </main>
  );
}

/** The add/edit form. Only the fields the chosen kind actually uses. */
function AdapterForm({
  value,
  busy,
  onChange,
  onSubmit,
  onCancel,
}: {
  value: AdapterInput;
  busy: boolean;
  onChange: (next: AdapterInput) => void;
  onSubmit: () => void;
  onCancel: () => void;
}) {
  const isJson = value.kind === "local-json";

  return (
    <form
      className="adapters__form"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit();
      }}
    >
      <label className="adapters__field">
        名称
        <input
          value={value.name}
          onChange={(event) => onChange({ ...value, name: event.target.value })}
          placeholder="my-harness"
        />
      </label>

      <label className="adapters__field">
        类型
        <select
          value={value.kind}
          onChange={(event) => onChange({ ...value, kind: event.target.value as AdapterKind })}
        >
          <option value="local-json">本地 JSON 文件</option>
          <option value="local-http">本地 HTTP 接口</option>
        </select>
      </label>

      {isJson ? (
        <label className="adapters__field">
          路径
          <input
            value={value.path ?? ""}
            onChange={(event) => onChange({ ...value, path: event.target.value })}
            placeholder="C:/status/harness.json"
          />
        </label>
      ) : (
        <>
          <label className="adapters__field">
            端口
            <input
              type="number"
              value={value.port ?? ""}
              onChange={(event) =>
                onChange({
                  ...value,
                  port: event.target.value === "" ? null : Number(event.target.value),
                })
              }
            />
          </label>
          <label className="adapters__field">
            路径
            <input
              value={value.http_path ?? ""}
              onChange={(event) => onChange({ ...value, http_path: event.target.value })}
              placeholder="/api/harness/snapshot"
            />
          </label>
        </>
      )}

      <div className="adapters__form-actions">
        <button type="submit" disabled={busy}>
          保存
        </button>
        <button type="button" disabled={busy} onClick={onCancel}>
          取消
        </button>
      </div>
    </form>
  );
}
