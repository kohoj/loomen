const app = document.querySelector("#app");
if (!app) throw new Error("Missing #app");

let snapshot = { dbPath: "", repos: [] };
let settings = null;
let selection = {};
let view = "workbench";
let settingsTab = "general";
let addRepoOpen = false;
let newWorkspaceOpen = false;
let weavePreview = null;
let weavePreviewKey = "";
let activeMainTab = "chat";
let runScriptEditorOpen = false;
let setupScriptEditorOpen = false;
let collapsedFileDirs = new Set();
let notificationsOpen = false;
let pending = false;
let launchHealth = null;
let lastDiff = "";
let terminalRun = null;
let ptyTerminals = [];
let selectedTerminalId = "";
let ptyPoller = null;
let draftPrompt = "";
let files = [];
let fileFilter = "";
let filePreview = null;
let selectedPreviewLine = 0;
let workspaceSearchQuery = "";
let workspaceSearchResults = [];
let workspaceSearchPending = false;
let changes = [];
let diffFiles = [];
let diffComments = [];
let selectedDiffPath = "";
let selectedHunkIndex = 0;
let selectedCommentLine = 0;
let changeFilter = "";
let activeRightPanel = "files";
let activeRunTab = "run";
let lifecycleRun = null;
let spotlighter = null;
let pendingSessionId = null;
let lastError = "";
let prInfo = null;
let prModalOpen = false;
let workspaceInitInfo = null;
let commandPaletteOpen = false;
let paletteQuery = "";
let paletteSelectedIndex = 0;
let notificationLog = loadNotificationLog();
let liveMessages = {};
let liveSessionEvents = {};
let listenersReady = false;
let contextUsage = null;
let toolApprovals = [];

async function invoke(command, args = {}) {
  if (!window.__TAURI_INTERNALS__?.invoke) {
    throw new Error("Tauri IPC is unavailable. Launch this through cargo run from src-tauri.");
  }
  return window.__TAURI_INTERNALS__.invoke(command, args);
}

async function listen(event, handler) {
  if (window.__TAURI__?.event?.listen) {
    return window.__TAURI__.event.listen(event, handler);
  }
  const callback = window.__TAURI_INTERNALS__.transformCallback(handler);
  const eventId = await invoke("plugin:event|listen", {
    event,
    target: { kind: "Any" },
    handler: callback
  });
  return async () => {
    window.__TAURI_EVENT_PLUGIN_INTERNALS__?.unregisterListener?.(event, eventId);
    await invoke("plugin:event|unlisten", { event, eventId }).catch(() => {});
  };
}

async function load() {
  await setupEventListeners();
  [snapshot, settings, launchHealth] = await Promise.all([
    invoke("get_state"),
    invoke("get_settings"),
    invoke("get_launch_health").catch(fallbackLaunchHealth)
  ]);
  selectFallbacks();
  await refreshWorkspacePanels();
  render();
}

async function setupEventListeners() {
  if (listenersReady) return;
  listenersReady = true;
  await listen("loomen-query-started", (event) => {
    const sessionId = event.payload?.sessionId;
    if (!sessionId) return;
    liveMessages[sessionId] = [];
    liveSessionEvents[sessionId] = [];
    pending = true;
    pendingSessionId = sessionId;
    pushNotification("Agent started", currentSession()?.title || sessionId, "info");
    render();
  });
  await listen("loomen-query-message", (event) => {
    const sessionId = event.payload?.sessionId;
    const text = event.payload?.text;
    if (!sessionId || !text) return;
    liveMessages[sessionId] = [...(liveMessages[sessionId] ?? []), text];
    if (selection.sessionId === sessionId) render();
  });
  await listen("loomen-query-session-event", (event) => {
    const sessionId = event.payload?.sessionId;
    const sessionEvent = event.payload?.event;
    if (!sessionId || !sessionEvent) return;
    liveSessionEvents[sessionId] = [...(liveSessionEvents[sessionId] ?? []), sessionEvent].slice(-30);
    if (selection.sessionId === sessionId) render();
  });
  await listen("loomen-query-finished", async (event) => {
    const sessionId = event.payload?.sessionId;
    if (!sessionId) return;
    if (event.payload?.error) {
      lastError = event.payload.error;
      pushNotification("Agent stopped with an error", lastError, "error");
    } else {
      pushNotification("Agent finished", currentSession()?.title || sessionId, "success");
    }
    pending = false;
    pendingSessionId = null;
    snapshot = await invoke("get_state").catch(() => snapshot);
    selectFallbacks();
    await refreshWorkspacePanels();
    delete liveMessages[sessionId];
    delete liveSessionEvents[sessionId];
    render();
  });
  await listen("loomen-tool-approval-requested", (event) => {
    if (!event.payload?.approvalId) return;
    toolApprovals = [
      ...toolApprovals.filter((approval) => approval.approvalId !== event.payload.approvalId),
      event.payload
    ];
    pushNotification("Tool approval required", event.payload.toolName || "tool", "warning");
    render();
  });
  window.addEventListener("keydown", handleGlobalKeys);
}

function selectFallbacks() {
  const repo = snapshot.repos.find((item) => item.id === selection.repoId) ?? snapshot.repos[0];
  selection.repoId = repo?.id;
  const workspace =
    repo?.workspaces.find((item) => item.id === selection.workspaceId) ?? repo?.workspaces[0];
  selection.workspaceId = workspace?.id;
  const session =
    workspace?.sessions.find((item) => item.id === selection.sessionId) ?? workspace?.sessions[0];
  selection.sessionId = session?.id;
}

function currentRepo() {
  return snapshot.repos.find((repo) => repo.id === selection.repoId);
}

function currentWorkspace() {
  return currentRepo()?.workspaces.find((workspace) => workspace.id === selection.workspaceId);
}

function currentSession() {
  return currentWorkspace()?.sessions.find((session) => session.id === selection.sessionId);
}

function recentSessions(limit = 8) {
  return snapshot.repos
    .flatMap((repo) =>
      repo.workspaces.flatMap((workspace) =>
        workspace.sessions.map((session) => ({
          repo,
          workspace,
          session
        }))
      )
    )
    .sort((a, b) => (b.session.updatedAt ?? 0) - (a.session.updatedAt ?? 0))
    .slice(0, limit);
}

function pushNotification(title, detail = "", tone = "info") {
  notificationLog = [
    {
      id: `${Date.now()}-${Math.random().toString(16).slice(2)}`,
      title,
      detail,
      tone,
      time: Date.now()
    },
    ...notificationLog
  ].slice(0, 40);
  persistNotificationLog();
}

function loadNotificationLog() {
  try {
    const raw = window.localStorage?.getItem("loomen.notifications");
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.slice(0, 40) : [];
  } catch {
    return [];
  }
}

function persistNotificationLog() {
  try {
    window.localStorage?.setItem("loomen.notifications", JSON.stringify(notificationLog));
  } catch {
    // Ignore storage failures; notifications are opportunistic UI state.
  }
}

function notificationItems() {
  const active = [];
  if (pending && pendingSessionId) {
    active.push({
      id: "pending-query",
      title: "Agent is working",
      detail: currentSession()?.title || pendingSessionId,
      tone: "info",
      time: Date.now()
    });
  }
  for (const approval of toolApprovals) {
    active.push({
      id: `approval-${approval.approvalId}`,
      title: "Tool approval required",
      detail: approval.toolName || "tool",
      tone: "warning",
      time: Date.now()
    });
  }
  if (lastError) {
    active.push({
      id: "last-error",
      title: "Last error",
      detail: lastError,
      tone: "error",
      time: Date.now()
    });
  }
  return [...active, ...notificationLog];
}

function notificationBadgeCount() {
  return notificationItems().filter((item) => item.tone === "error" || item.tone === "warning" || item.id === "pending-query").length;
}

function fallbackLaunchHealth(error) {
  return {
    status: "error",
    generatedAt: Date.now(),
    dbPath: snapshot.dbPath || "",
    rebuildRoot: "",
    checks: [
      {
        id: "launchHealth",
        label: "Launch health",
        status: "error",
        detail: String(error),
        path: null,
        version: null,
        required: true,
        remediation: "Restart Loomen or inspect the Tauri command logs."
      }
    ]
  };
}

async function refreshLaunchHealth() {
  launchHealth = await invoke("get_launch_health").catch(fallbackLaunchHealth);
  render();
}

function renderWeavePreview() {
  if (!weavePreview) {
    return `<div class="weave-preview muted">Weave preview appears after you name the workspace.</div>`;
  }
  const rows = [
    ["Repository", weavePreview.repoName],
    ["Base branch", weavePreview.baseBranch],
    ["New branch", weavePreview.branchName],
    ["Worktree path", weavePreview.worktreePath],
    ["Checkpoint", weavePreview.checkpointId]
  ];
  return `
    <section class="weave-preview ${weavePreview.canCreate ? "" : "blocked"}">
      <header>
        <strong>${weavePreview.canCreate ? "Ready to weave" : "Weave needs attention"}</strong>
        <span>${weavePreview.pathExists ? (weavePreview.pathIsEmpty ? "path exists and is empty" : "path is occupied") : "path will be created"}</span>
      </header>
      <div>
        ${rows.map(([label, value]) => `<small>${escapeHtml(label)}</small><code>${escapeHtml(value || "unknown")}</code>`).join("")}
      </div>
      ${(weavePreview.warnings || []).map((warning) => `<p>${escapeHtml(warning)}</p>`).join("")}
    </section>
  `;
}

function render() {
  if (view === "settings") {
    app.innerHTML = renderSettings();
    bindSettingsEvents();
    return;
  }

  const repo = currentRepo();
  const workspace = currentWorkspace();
  const session = currentSession();

  app.innerHTML = `
    <main class="shell">
      <aside class="rail">
        <div class="rail-toolbar" aria-label="Global controls">
          <button type="button" class="icon-button ${notificationsOpen ? "active" : ""}" title="Notifications" aria-label="Notifications" data-action-click="toggle-notifications">
            T${notificationBadgeCount() ? `<i>${escapeHtml(notificationBadgeCount())}</i>` : ""}
          </button>
          <button type="button" class="icon-button" title="Command palette" aria-label="Command palette" data-action-click="open-palette">K</button>
          <button type="button" class="icon-button" title="Settings" aria-label="Settings" data-action-click="open-settings">,</button>
        </div>
        ${renderNotificationsDrawer()}
        <div class="brand compact-brand">
          <div class="mark">L</div>
          <strong>Loomen</strong>
        </div>
        <div class="section-title">History</div>
        <div class="list compact history-list">
          ${
            recentSessions()
              .map(
                ({ repo, workspace, session }) => `
                  <button class="row ${session.id === selection.sessionId ? "active" : ""}" data-history-session="${escapeAttr(session.id)}" data-history-workspace="${escapeAttr(workspace.id)}" data-history-repo="${escapeAttr(repo.id)}">
                    <span>${escapeHtml(session.title)}</span>
                    <small>${escapeHtml(session.agentType)} · ${escapeHtml(workspace.name)} · ${escapeHtml(repo.name)}</small>
                  </button>
                `
              )
              .join("") || `<div class="muted-row">No chat history yet</div>`
          }
        </div>
        <div class="section-title section-title-row">
          <span>Workspaces</span>
          <button type="button" class="mini-add" title="New workspace" aria-label="New workspace" data-action-click="toggle-new-workspace" ${repo ? "" : "disabled"}>+</button>
        </div>
        ${
          newWorkspaceOpen
            ? `
              <form class="workspace-form inline-workspace-form" data-action="create-workspace">
                <input name="name" placeholder="Workspace name" ${repo ? "" : "disabled"} />
                ${renderBaseBranchSelect(repo)}
                <input name="path" placeholder="Optional worktree path" ${repo ? "" : "disabled"} />
                ${renderWeavePreview()}
                <button type="submit" ${repo && (!weavePreview || weavePreview.canCreate) ? "" : "disabled"}>Weave workspace</button>
              </form>
            `
            : `<button type="button" class="repo-add-row" data-action-click="toggle-new-workspace" ${repo ? "" : "disabled"}>Weave workspace from ${escapeHtml(repo?.currentBranch || repo?.defaultBranch || "HEAD")}</button>`
        }
        <div class="list compact workspace-quick-list">
          ${
            repo?.workspaces
              .map(
                (item) => `
                  <button class="row ${item.id === selection.workspaceId ? "active" : ""}" data-select-workspace="${item.id}">
                    <span>${escapeHtml(item.name)}</span>
                    <small>${escapeHtml(item.branchName || "main")} · ${escapeHtml(item.state)}</small>
                  </button>
                `
              )
              .join("") || `<div class="muted-row">No workspaces</div>`
          }
        </div>
        <div class="section-title section-title-row">
          <span>Repositories</span>
          <button type="button" class="mini-add" title="Add repository" aria-label="Add repository" data-action-click="toggle-add-repo">+</button>
        </div>
        ${
          addRepoOpen
            ? `
              <form class="add-repo" data-action="add-repo">
                <input name="path" placeholder="Repo path, e.g. ~/Projects/app" />
                <button type="submit">Add</button>
              </form>
            `
            : `<button type="button" class="repo-add-row" data-action-click="toggle-add-repo">Add repository</button>`
        }
        <div class="list">
          ${snapshot.repos
            .map(
              (item) => `
                <button class="row ${item.id === selection.repoId ? "active" : ""}" data-select-repo="${item.id}">
                  <span>${escapeHtml(item.name)}</span>
                  <small>${escapeHtml(item.currentBranch || item.defaultBranch || "no branch")} · ${escapeHtml(item.path)}</small>
                </button>
              `
            )
            .join("")}
        </div>
        ${
          repo
            ? `
              <div class="repo-tools">
                <button type="button" data-action-click="open-current-repo-settings">Repo settings</button>
                <button type="button" data-action-click="open-repo-finder">Open repo</button>
              </div>
            `
            : ""
        }
        <div class="rail-footer">
          <button class="settings-entry" data-action-click="open-settings">Settings</button>
        </div>
      </aside>

      <section class="workspace-pane">
        <header class="pane-header">
          <div>
            <h1>${escapeHtml(repo?.name ?? "No repository")}</h1>
            <p>${escapeHtml(repo?.path ?? "Add a local git repository to begin.")}</p>
            <p>${escapeHtml([repo?.currentBranch, repo?.defaultBranch, repo?.remote].filter(Boolean).join(" · "))}</p>
          </div>
          <code>${escapeHtml(snapshot.dbPath)}</code>
        </header>
        <div class="section-title">Workspaces</div>
        <div class="list compact">
          ${
            repo?.workspaces
              .map(
                (item) => `
                  <button class="row ${item.id === selection.workspaceId ? "active" : ""}" data-select-workspace="${item.id}">
                    <span>${escapeHtml(item.name)}</span>
                    <small>${escapeHtml(item.branchName || "main")} · ${escapeHtml(item.state)} · ${escapeHtml(item.path)}</small>
                  </button>
                `
              )
              .join("") ?? ""
          }
        </div>
        <form class="workspace-form" data-action="create-workspace">
          <input name="name" placeholder="Workspace name" ${repo ? "" : "disabled"} />
          ${renderBaseBranchSelect(repo)}
          <input name="path" placeholder="Optional worktree path" ${repo ? "" : "disabled"} />
          ${renderWeavePreview()}
          <button type="submit" ${repo && (!weavePreview || weavePreview.canCreate) ? "" : "disabled"}>Weave workspace</button>
        </form>
      </section>

      <section class="chat-pane">
        <div class="workbench-main">
          <header class="chat-header">
            <div class="workspace-title">
              <div class="title-line">
                <h2>${escapeHtml(workspace ? `${repo?.name ?? "repo"}/${workspace.name}` : "No workspace selected")}</h2>
                ${workspace?.state ? `<span class="state-pill">${escapeHtml(workspace.state)}</span>` : ""}
              </div>
              <div class="workspace-meta">
                <button type="button" class="meta-control" title="Base branch">${escapeHtml(workspace?.baseBranch || repo?.defaultBranch || "origin/main")}</button>
                <button type="button" class="meta-control path-control" title="${escapeAttr(workspace?.path || repo?.path || "")}" data-action-click="open-workspace-finder">${escapeHtml(shortPath(workspace?.path || repo?.path || ""))}</button>
                ${workspace?.checkpointId ? `<span class="checkpoint-dot" title="${escapeAttr(workspace.checkpointId)}">checkpoint</span>` : ""}
              </div>
            </div>
            <div class="actions">
              <button class="compact-action" title="Save checkpoint" aria-label="Save checkpoint" data-action-click="checkpoint" ${workspace ? "" : "disabled"}>Save</button>
              <button class="compact-action" title="Show diff" aria-label="Show diff" data-action-click="diff" ${workspace ? "" : "disabled"}>Diff</button>
              <button class="compact-action" title="Archive workspace" aria-label="Archive workspace" data-action-click="archive" ${workspace && workspace.state !== "archived" ? "" : "disabled"}>Archive</button>
              <button class="compact-action" title="Restore workspace" aria-label="Restore workspace" data-action-click="restore" ${workspace?.state === "archived" ? "" : "disabled"}>Restore</button>
              <button class="compact-action primary-action" title="New Claude chat" aria-label="New Claude chat" data-action-click="new-claude" ${workspace ? "" : "disabled"}>Claude</button>
              <button class="compact-action" title="New Codex chat" aria-label="New Codex chat" data-action-click="new-codex" ${workspace ? "" : "disabled"}>Codex</button>
            </div>
          </header>
          <div class="tabs">
            <button class="tab ${activeMainTab === "scratchpad" ? "active" : ""}" data-center-tab="scratchpad"><span>notes</span>Scratchpad</button>
            ${
              workspace?.sessions
                .map(
                  (item) => `
                    <button class="tab ${activeMainTab === "chat" && item.id === selection.sessionId ? "active" : ""}" data-select-session="${item.id}">
                      <span>${escapeHtml(item.agentType)}</span>
                      ${escapeHtml(item.title)}
                      <b data-close-session="${escapeAttr(item.id)}">×</b>
                    </button>
                  `
                )
                .join("") ?? ""
            }
            <button class="tab add-tab" data-action-click="new-claude">+</button>
          </div>
          <div class="messages">
            ${activeMainTab === "scratchpad" ? renderScratchpad(workspace) : renderConversation(session, workspace, repo)}
          </div>
          <form class="composer" data-action="send-query">
            ${
              session
                ? `
                  <div class="composer-toolbar" data-action="session-settings">
                    <span class="agent-selector">${escapeHtml(agentLabel(session.agentType))}</span>
                    <label class="pill-field model-pill">
                      <span>${escapeHtml(modelLabel(session.agentType))}</span>
                      <select name="model">${modelOptions(session)}</select>
                    </label>
                    <span class="effort-pill">${escapeHtml(effortLabel(session.agentType))}</span>
                    <label class="pill-field permission-pill">
                      <span>Mode</span>
                      <select name="permissionMode">
                        ${permissionOptions(session.permissionMode)}
                      </select>
                    </label>
                    ${renderContextUsage()}
                    <span class="focus-hint">⌘L to focus</span>
                  </div>
                `
                : ""
            }
            <textarea name="prompt" rows="3" placeholder="Ask to make changes, @mention files, run /commands" ${session || pending ? "" : "disabled"}>${escapeHtml(draftPrompt)}</textarea>
            <div class="composer-suggestions-slot">
              ${renderComposerSuggestions(session)}
            </div>
            <button type="submit" ${session && !pending ? "" : "disabled"}>${pending ? "Sending" : "Send"}</button>
            <button type="button" data-action-click="cancel-query" ${pending && pendingSessionId ? "" : "disabled"}>Cancel</button>
          </form>
        </div>
        <aside class="inspector">
          <div class="panel-top">
            ${renderPrSummary()}
            <div class="segmented">
              <button class="${activeRightPanel === "files" ? "active" : ""}" data-panel="files">All files <span>${escapeHtml(files.length)}</span></button>
              <button class="${activeRightPanel === "changes" ? "active" : ""}" data-panel="changes">Changes <span>${escapeHtml(changes.length || diffFiles.length)}</span></button>
              <button class="${activeRightPanel === "checks" ? "active" : ""}" data-panel="checks">Checks <span>${escapeHtml(prInfo?.checks?.length ?? 0)}</span></button>
            </div>
          </div>
          <div class="file-list">
            ${renderRightPanel()}
          </div>
          ${renderRunPanel(repo, workspace)}
        </aside>
      </section>
    </main>
    ${renderCommandPalette()}
    ${renderPrCreateModal()}
    ${renderToolApprovalModal()}
  `;

  bindEvents();
}

function bindEvents() {
  document.querySelectorAll("[data-command-action]").forEach((button) => {
    button.addEventListener("click", () => runCommandAction(button.dataset.commandAction));
  });
  bindComposerSuggestionEvents();
  document.querySelectorAll("[data-select-repo]").forEach((button) => {
    button.addEventListener("click", async () => {
      selection = { repoId: button.dataset.selectRepo };
      selectFallbacks();
      await refreshWorkspacePanels();
      render();
    });
  });
  document.querySelectorAll("[data-select-workspace]").forEach((button) => {
    button.addEventListener("click", async () => {
      selection.workspaceId = button.dataset.selectWorkspace;
      selection.sessionId = undefined;
      activeMainTab = "chat";
      selectFallbacks();
      await refreshWorkspacePanels();
      render();
    });
  });
  document.querySelectorAll("[data-history-session]").forEach((button) => {
    button.addEventListener("click", async () => {
      selection.repoId = button.dataset.historyRepo;
      selection.workspaceId = button.dataset.historyWorkspace;
      selection.sessionId = button.dataset.historySession;
      activeMainTab = "chat";
      await refreshWorkspacePanels();
      render();
    });
  });
  document.querySelectorAll("[data-select-session]").forEach((button) => {
    button.addEventListener("click", (event) => {
      if (event.target?.dataset?.closeSession) return;
      selection.sessionId = button.dataset.selectSession;
      activeMainTab = "chat";
      render();
    });
  });
  document.querySelectorAll("[data-center-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      activeMainTab = button.dataset.centerTab || "chat";
      render();
    });
  });
  document.querySelectorAll("[data-close-session]").forEach((button) => {
    button.addEventListener("click", async (event) => {
      event.stopPropagation();
      await closeSession(button.dataset.closeSession);
    });
  });

  document.querySelector('[data-action="add-repo"]')?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const path = new FormData(event.currentTarget).get("path")?.toString().trim();
    if (!path) return;
    snapshot = await invoke("add_repo", { path });
    selection = {};
    addRepoOpen = false;
    activeMainTab = "chat";
    selectFallbacks();
    await refreshWorkspacePanels();
    render();
  });

  document.querySelector('[data-action="create-workspace"]')?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const repo = currentRepo();
    if (!repo) return;
    const form = new FormData(event.currentTarget);
    snapshot = await invoke("create_workspace", {
      repoId: repo.id,
      name: form.get("name")?.toString().trim() || "workspace",
      path: form.get("path")?.toString().trim() || repo.path,
      baseBranch: form.get("baseBranch")?.toString().trim() || repo.currentBranch || repo.defaultBranch || "HEAD"
    });
    newWorkspaceOpen = false;
    weavePreview = null;
    weavePreviewKey = "";
    activeMainTab = "chat";
    selectFallbacks();
    await refreshWorkspacePanels();
    render();
  });
  document.querySelectorAll('[data-action="create-workspace"] input, [data-action="create-workspace"] select').forEach((element) => {
    element.addEventListener("input", refreshWeavePreviewFromForm);
    element.addEventListener("change", refreshWeavePreviewFromForm);
  });

  bindActionClicks("new-claude", () => newSession("claude"));
  bindActionClicks("new-codex", () => newSession("codex"));
  bindActionClicks("toggle-notifications", () => {
    notificationsOpen = !notificationsOpen;
    render();
  });
  bindActionClicks("clear-notifications", () => {
    notificationLog = [];
    lastError = "";
    persistNotificationLog();
    render();
  });
  bindActionClicks("toggle-add-repo", () => {
    addRepoOpen = !addRepoOpen;
    render();
    if (addRepoOpen) document.querySelector('[data-action="add-repo"] input[name="path"]')?.focus();
  });
  bindActionClicks("toggle-new-workspace", () => {
    newWorkspaceOpen = !newWorkspaceOpen;
    render();
    if (newWorkspaceOpen) {
      const nameInput = document.querySelector('[data-action="create-workspace"] input[name="name"]');
      nameInput?.focus();
      refreshWeavePreviewFromForm({ currentTarget: nameInput });
    } else {
      weavePreview = null;
      weavePreviewKey = "";
    }
  });
  bindActionClicks("open-palette", () => {
    paletteQuery = "";
    paletteSelectedIndex = 0;
    commandPaletteOpen = true;
    render();
  });
  bindActionClicks("open-settings", () => {
    view = "settings";
    render();
  });
  bindActionClicks("open-current-repo-settings", () => {
    openCurrentRepoSettings();
  });
  bindActionClicks("open-workspace-finder", openWorkspaceInFinder);
  bindActionClicks("open-repo-finder", openRepoInFinder);
  document.querySelectorAll("[data-quick-prompt]").forEach((button) => {
    button.addEventListener("click", () => {
      draftPrompt = button.dataset.quickPrompt || "";
      activeMainTab = "chat";
      render();
      document.querySelector('[data-action="send-query"] textarea[name="prompt"]')?.focus();
    });
  });
  bindActionClicks("checkpoint", saveCheckpoint);
  bindActionClicks("diff", showDiff);
  bindActionClicks("archive", archiveCurrentWorkspace);
  bindActionClicks("restore", restoreCurrentWorkspace);
  bindActionClicks("run-setup", runWorkspaceSetup);
  bindActionClicks("run-script", runWorkspaceScript);
  bindActionClicks("start-spotlight", startSpotlight);
  bindActionClicks("stop-spotlight", stopSpotlight);
  bindActionClicks("cancel-query", cancelCurrentQuery);
  bindActionClicks("refresh-pr", refreshPullRequestInfo);
  bindActionClicks("rerun-failed-checks", rerunFailedChecks);
  bindActionClicks("add-run-script", () => {
    runScriptEditorOpen = true;
    activeRunTab = "run";
    render();
  });
  bindActionClicks("add-setup-script", () => {
    setupScriptEditorOpen = true;
    activeRunTab = "setup";
    render();
  });
  bindActionClicks("edit-run-script", () => {
    runScriptEditorOpen = true;
    activeRunTab = "run";
    render();
  });
  bindActionClicks("edit-setup-script", () => {
    setupScriptEditorOpen = true;
    activeRunTab = "setup";
    render();
  });
  bindActionClicks("open-pr-modal", () => {
    prModalOpen = true;
    render();
  });
  document.querySelectorAll("[data-approval-decision]").forEach((button) => {
    button.addEventListener("click", () => resolveToolApproval(button.dataset.approvalDecision, button.dataset.approvalId));
  });
  document.querySelector('[data-action="palette-search"]')?.addEventListener("input", (event) => {
    paletteQuery = event.currentTarget.value;
    paletteSelectedIndex = 0;
    const cursor = event.currentTarget.selectionStart ?? paletteQuery.length;
    render();
    requestAnimationFrame(() => {
      const input = document.querySelector('[data-action="palette-search"]');
      input?.focus();
      input?.setSelectionRange?.(cursor, cursor);
    });
  });
  document.querySelector('[data-action="palette-search"]')?.addEventListener("keydown", (event) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      movePaletteSelection(1);
      return;
    }
    if (event.key === "ArrowUp") {
      event.preventDefault();
      movePaletteSelection(-1);
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      document.querySelector(`[data-command-index="${paletteSelectedIndex}"]`)?.click();
    }
  });
  document.querySelectorAll("[data-run-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      activeRunTab = button.dataset.runTab || "run";
      render();
    });
  });
  document.querySelectorAll("[data-terminal-id]").forEach((button) => {
    button.addEventListener("click", (event) => {
      if (event.target?.dataset?.closeTerminal) return;
      selectedTerminalId = button.dataset.terminalId;
      render();
    });
  });
  document.querySelectorAll("[data-close-terminal]").forEach((button) => {
    button.addEventListener("click", async (event) => {
      event.stopPropagation();
      await closePtyTerminal(button.dataset.closeTerminal);
    });
  });
  document.querySelector('[data-action="save-scripts"]')?.addEventListener("submit", saveRepoScripts);
  document.querySelectorAll("[data-panel]").forEach((button) => {
    button.addEventListener("click", async () => {
      activeRightPanel = button.dataset.panel;
      await refreshWorkspacePanels();
      render();
    });
  });
  document.querySelectorAll("[data-diff-file]").forEach((button) => {
    button.addEventListener("click", () => {
      selectedDiffPath = button.dataset.diffFile;
      selectedHunkIndex = 0;
      selectedCommentLine = 0;
      render();
    });
  });
  document.querySelectorAll("[data-select-hunk]").forEach((button) => {
    button.addEventListener("click", () => {
      selectedHunkIndex = Number(button.dataset.selectHunk || 0);
      selectedCommentLine = 0;
      render();
    });
  });
  document.querySelectorAll("[data-comment-line]").forEach((button) => {
    button.addEventListener("click", () => {
      selectedCommentLine = Number(button.dataset.commentLine || 0);
      render();
    });
  });
  document.querySelector('[data-action="change-filter"]')?.addEventListener("input", (event) => {
    changeFilter = event.currentTarget.value;
    const cursor = event.currentTarget.selectionStart ?? changeFilter.length;
    render();
    requestAnimationFrame(() => {
      const input = document.querySelector('[data-action="change-filter"]');
      input?.focus();
      input?.setSelectionRange?.(cursor, cursor);
    });
  });
  bindActionClicks("copy-selected-patch", copySelectedPatch);
  bindActionClicks("open-selected-diff-file", openSelectedDiffFile);
  bindActionClicks("reveal-selected-diff-file", revealSelectedDiffFile);
  document.querySelectorAll("[data-open-file]").forEach((button) => {
    button.addEventListener("click", async () => {
      await openWorkspaceFile(button.dataset.openFile);
    });
  });
  document.querySelector('[data-action="file-filter"]')?.addEventListener("input", (event) => {
    fileFilter = event.currentTarget.value;
    const cursor = event.currentTarget.selectionStart ?? fileFilter.length;
    render();
    requestAnimationFrame(() => {
      const input = document.querySelector('[data-action="file-filter"]');
      input?.focus();
      input?.setSelectionRange?.(cursor, cursor);
    });
  });
  document.querySelector('[data-action="workspace-search"]')?.addEventListener("submit", searchWorkspaceContents);
  document.querySelectorAll("[data-search-result-path]").forEach((button) => {
    button.addEventListener("click", async () => {
      activeRightPanel = "files";
      await openWorkspaceFile(button.dataset.searchResultPath, Number(button.dataset.searchResultLine || 0));
    });
  });
  bindActionClicks("reveal-preview-file", revealPreviewFile);
  bindActionClicks("open-preview-file", openPreviewFileExternal);
  bindActionClicks("copy-preview-path", copyPreviewPath);
  document.querySelectorAll("[data-toggle-dir]").forEach((button) => {
    button.addEventListener("click", () => {
      const dir = button.dataset.toggleDir;
      if (!dir) return;
      if (collapsedFileDirs.has(dir)) {
        collapsedFileDirs.delete(dir);
      } else {
        collapsedFileDirs.add(dir);
      }
      render();
    });
  });
  document.querySelector('[data-action="add-comment"]')?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const workspace = currentWorkspace();
    if (!workspace) return;
    const body = new FormData(event.currentTarget).get("body")?.toString().trim();
    if (!body) return;
    diffComments = await invoke("add_diff_comment", {
      workspaceId: workspace.id,
      filePath: event.currentTarget.dataset.file,
      lineNumber: Number(event.currentTarget.dataset.lineNumber || 0),
      body
    });
    render();
  });
  document.querySelectorAll("[data-resolve-comment]").forEach((button) => {
    button.addEventListener("click", async () => {
      const workspace = currentWorkspace();
      if (!workspace) return;
      diffComments = await invoke("resolve_diff_comment", {
        workspaceId: workspace.id,
        commentId: button.dataset.resolveComment
      });
      render();
    });
  });
  document.querySelector('[data-action="notes"]')?.addEventListener("change", saveNotes);
  document.querySelector('[data-action="session-settings"]')?.addEventListener("change", saveSessionSettings);

  document.querySelector('[data-action="send-query"]')?.addEventListener("submit", async (event) => {
    event.preventDefault();
    await submitPromptForm(event.currentTarget);
  });
  const promptInput = document.querySelector('[data-action="send-query"] textarea[name="prompt"]');
  promptInput?.addEventListener("input", (event) => {
    draftPrompt = event.currentTarget.value;
    updateComposerSuggestions();
  });
  promptInput?.addEventListener("keydown", async (event) => {
    if (event.isComposing || !shouldSubmitComposer(event)) return;
    event.preventDefault();
    await submitPromptForm(event.currentTarget.form);
  });

  document.querySelector('[data-action="run-terminal"]')?.addEventListener("submit", async (event) => {
    event.preventDefault();
    const workspace = currentWorkspace();
    if (!workspace) return;
    const command = new FormData(event.currentTarget).get("command")?.toString().trim();
    if (!command) return;
    terminalRun = await invoke("run_terminal_command", { workspaceId: workspace.id, command });
    await refreshWorkspacePanels();
    render();
  });
  bindActionClicks("start-pty", startPtyTerminal);
  bindActionClicks("stop-pty", stopPtyTerminal);
  document.querySelector('[data-action="pty-send"]')?.addEventListener("submit", sendPtyInput);
  document.querySelector('[data-action="create-pr"]')?.addEventListener("submit", createPullRequest);
  bindActionClicks("close-pr-modal", () => {
    prModalOpen = false;
    render();
  });
}

function bindComposerSuggestionEvents(root = document) {
  root.querySelectorAll("[data-composer-suggestion]").forEach((button) => {
    button.addEventListener("click", () => applyComposerSuggestion(button.dataset.composerSuggestion || ""));
  });
}

function bindActionClicks(action, handler) {
  document.querySelectorAll(`[data-action-click="${action}"]`).forEach((button) => {
    button.addEventListener("click", handler);
  });
}

async function refreshWeavePreviewFromForm(event) {
  const form = event.currentTarget?.form;
  const repo = currentRepo();
  if (!form || !repo) return;
  const data = new FormData(form);
  const name = data.get("name")?.toString().trim() || "workspace";
  const path = data.get("path")?.toString().trim() || "";
  const baseBranch = data.get("baseBranch")?.toString().trim() || repo.currentBranch || repo.defaultBranch || "HEAD";
  const key = [repo.id, name, path, baseBranch].join("\n");
  weavePreviewKey = key;
  const preview = await invoke("preview_workspace", {
    repoId: repo.id,
    name,
    path,
    baseBranch
  }).catch((error) => ({
    canCreate: false,
    repoName: repo.name,
    baseBranch,
    branchName: "unknown",
    worktreePath: path || repo.path,
    checkpointId: "unknown",
    pathExists: false,
    pathIsEmpty: false,
    warnings: [String(error)]
  }));
  if (weavePreviewKey !== key) return;
  weavePreview = preview;
  render();
}

function renderEmptySession(workspace, repo) {
  if (!workspace) return `<div class="empty">Create a workspace and start a chat.</div>`;
  const branch = workspace.branchName || repo?.currentBranch || "branch";
  const fileCount = files.length ? `copied ${formatNumber(files.length)} files` : "ready to inspect files";
  return `
    <section class="onboarding">
      <h3>You’re in a new copy of ${escapeHtml(repo?.name || "this repo")} called ${escapeHtml(workspace.name)}</h3>
      <div class="branch-pill">Branched ${escapeHtml(branch)} from ${escapeHtml(workspace.baseBranch || repo?.defaultBranch || "base")}</div>
      <p>Created <strong>${escapeHtml(workspace.name)}</strong> and ${escapeHtml(fileCount)}</p>
      <div class="starter-actions">
        <button type="button" data-quick-prompt="Optional: add a setup script for this repository and explain what it runs.">Optional: add a setup script</button>
        <button type="button" data-quick-prompt="Run the project’s security audit or dependency audit and summarize the result.">Run security audit</button>
        <button type="button" data-quick-prompt="Improve the repository’s CLAUDE.md or agent instructions based on the codebase.">Improve CLAUDE.md</button>
        <button type="button" data-quick-prompt="Find an actionable TODO in this repository and implement it.">Solve a TODO</button>
      </div>
    </section>
  `;
}

function renderScratchpad(workspace) {
  if (!workspace) return `<div class="empty">Create a workspace and use Scratchpad notes.</div>`;
  return `
    <section class="scratchpad-panel">
      <textarea class="scratchpad" data-action="notes" placeholder="Reference with @notes">${escapeHtml(workspace.notes ?? "")}</textarea>
    </section>
  `;
}

function renderConversation(session, workspace, repo) {
  return `
    ${lastDiff ? `<article class="message system"><strong>diff</strong><pre>${escapeHtml(lastDiff)}</pre></article>` : ""}
    ${
      terminalRun
        ? `<article class="message system"><strong>terminal · exit ${escapeHtml(String(terminalRun.exitCode ?? "signal"))}</strong><pre>${escapeHtml(`$ ${terminalRun.command}\n${terminalRun.output}`)}</pre></article>`
        : ""
    }
    ${lastError ? `<article class="message system error"><strong>error</strong><pre>${escapeHtml(lastError)}</pre></article>` : ""}
    ${
      session?.messages.length
        ? session.messages
            .map(
              (message) => `
                <article class="message ${message.role}">
                  <strong>${escapeHtml(message.role)}</strong>
                  <pre>${escapeHtml(message.content)}</pre>
                </article>
              `
            )
            .join("")
        : renderEmptySession(workspace, repo)
    }
    ${renderLiveMessage(session)}
    ${renderLiveSessionEvents(session)}
  `;
}

function renderBaseBranchSelect(repo) {
  if (!repo) return `<select name="baseBranch" disabled><option>HEAD</option></select>`;
  const options = [...new Set([repo.currentBranch, repo.defaultBranch, ...(repo.branches || []), "HEAD"].filter(Boolean))];
  return `
    <select name="baseBranch" title="Base branch">
      ${options
        .map(
          (branch) => `<option value="${escapeAttr(branch)}" ${branch === repo.currentBranch ? "selected" : ""}>${escapeHtml(branch)}</option>`
        )
        .join("")}
    </select>
  `;
}

function renderNotificationsDrawer() {
  if (!notificationsOpen) return "";
  const items = notificationItems();
  return `
    <section class="notifications-drawer">
      <header>
        <strong>Notifications</strong>
        <div>
          <button type="button" title="Clear notifications" aria-label="Clear notifications" data-action-click="clear-notifications">Clear</button>
          <button type="button" title="Close notifications" aria-label="Close notifications" data-action-click="toggle-notifications">x</button>
        </div>
      </header>
      <div class="notifications-list">
        ${
          items.length
            ? items
                .slice(0, 12)
                .map(
                  (item) => `
                    <article class="notification-item ${escapeAttr(item.tone || "info")}">
                      <strong>${escapeHtml(item.title)}</strong>
                      <span>${escapeHtml(item.detail || relativeTime(item.time))}</span>
                    </article>
                  `
                )
                .join("")
            : `<div class="notification-empty">No notifications</div>`
        }
      </div>
    </section>
  `;
}

function renderRunPanel(repo, workspace) {
  const currentTerminal = currentPtyTerminal();
  return `
    <div class="run-panel">
      <div class="run-tabs">
        <button class="${activeRunTab === "setup" ? "active" : ""}" data-run-tab="setup">Setup</button>
        <button class="${activeRunTab === "run" ? "active" : ""}" data-run-tab="run">Run</button>
        <button class="${activeRunTab === "terminal" ? "active" : ""}" data-run-tab="terminal">Terminal</button>
        <button data-action-click="start-pty" ${workspace ? "" : "disabled"}>+</button>
      </div>
      ${activeRunTab === "setup" ? renderSetupTab(repo, workspace) : ""}
      ${activeRunTab === "run" ? renderRunTab(repo, workspace) : ""}
      ${activeRunTab === "terminal" ? renderTerminalTab(workspace, currentTerminal) : ""}
    </div>
  `;
}

function renderSetupTab(repo, workspace) {
  if (!repo?.setupScript && !setupScriptEditorOpen) {
    return `
      <div class="panel-empty run-empty">
        <button type="button" data-action-click="add-setup-script">Add setup script</button>
        <span>Prepare dependencies or bootstrap the workspace before the first agent run</span>
      </div>
    `;
  }
  if (repo?.setupScript && !setupScriptEditorOpen) {
    const firstLine = repo.setupScript.split("\n").find((line) => line.trim()) || "Setup script configured";
    return `
      <div class="script-summary">
        <div>
          <strong>Setup script</strong>
          <code>${escapeHtml(firstLine)}</code>
        </div>
        <button type="button" data-action-click="edit-setup-script">Edit</button>
      </div>
      <div class="lifecycle-actions">
        <button data-action-click="run-setup" ${workspace ? "" : "disabled"}>Run setup</button>
      </div>
      ${lifecycleRun ? `<pre class="lifecycle-output">${escapeHtml(`$ ${lifecycleRun.command}\n${lifecycleRun.output}`)}</pre>` : `<div class="panel-empty small">No setup output yet</div>`}
    `;
  }
  return `
    <form class="script-config" data-action="save-scripts">
      <textarea name="setupScript" rows="4" placeholder="Setup script">${escapeHtml(repo?.setupScript ?? "")}</textarea>
      <button type="submit" ${repo ? "" : "disabled"}>Save setup</button>
    </form>
    <div class="lifecycle-actions">
      <button data-action-click="run-setup" ${workspace && repo?.setupScript ? "" : "disabled"}>Run setup</button>
    </div>
    ${lifecycleRun ? `<pre class="lifecycle-output">${escapeHtml(`$ ${lifecycleRun.command}\n${lifecycleRun.output}`)}</pre>` : `<div class="panel-empty small">No setup output yet</div>`}
  `;
}

function renderRunTab(repo, workspace) {
  if (!repo?.runScript && !runScriptEditorOpen) {
    return `
      <div class="panel-empty run-empty">
        <button type="button" data-action-click="add-run-script">Add run script</button>
        <span>Run tests or a development server to test changes in this workspace</span>
      </div>
    `;
  }
  if (repo?.runScript && !runScriptEditorOpen) {
    const firstLine = repo.runScript.split("\n").find((line) => line.trim()) || "Run script configured";
    return `
      <div class="script-summary">
        <div>
          <strong>Run script</strong>
          <code>${escapeHtml(firstLine)}</code>
        </div>
        <button type="button" data-action-click="edit-run-script">Edit</button>
      </div>
      <div class="lifecycle-actions">
        <button data-action-click="run-script" ${workspace ? "" : "disabled"}>Run script</button>
        <button data-action-click="start-spotlight" ${workspace && !spotlighter?.isRunning ? "" : "disabled"}>Spotlight</button>
        <button data-action-click="stop-spotlight" ${spotlighter?.isRunning ? "" : "disabled"}>Stop sync</button>
      </div>
      ${lifecycleRun ? `<pre class="lifecycle-output">${escapeHtml(`$ ${lifecycleRun.command}\n${lifecycleRun.output}`)}</pre>` : `<div class="panel-empty small">No run output yet</div>`}
    `;
  }
  return `
    <form class="script-config" data-action="save-scripts">
      <textarea name="runScript" rows="4" placeholder="Run script">${escapeHtml(repo?.runScript ?? "")}</textarea>
      <button type="submit" ${repo ? "" : "disabled"}>Save run script</button>
    </form>
    <div class="lifecycle-actions">
      <button data-action-click="run-script" ${workspace && repo?.runScript ? "" : "disabled"}>Run script</button>
      <button data-action-click="start-spotlight" ${workspace && !spotlighter?.isRunning ? "" : "disabled"}>Spotlight</button>
      <button data-action-click="stop-spotlight" ${spotlighter?.isRunning ? "" : "disabled"}>Stop sync</button>
    </div>
    ${lifecycleRun ? `<pre class="lifecycle-output">${escapeHtml(`$ ${lifecycleRun.command}\n${lifecycleRun.output}`)}</pre>` : `<div class="panel-empty small">Run tests or a development server to test changes in this workspace</div>`}
  `;
}

function renderTerminalTab(workspace, terminal) {
  return `
    <div class="terminal-tabs">
      ${
        ptyTerminals.length
          ? ptyTerminals
              .map(
                (item, index) => `
                  <button class="${item.id === terminal?.id ? "active" : ""}" data-terminal-id="${escapeAttr(item.id)}">
                    <span>${escapeHtml(item.isRunning ? "●" : "○")}</span>
                    Terminal ${index + 1}
                    <b data-close-terminal="${escapeAttr(item.id)}">×</b>
                  </button>
                `
              )
              .join("")
          : `<span class="terminal-empty-tab">No terminal tabs</span>`
      }
    </div>
    <form class="terminal" data-action="run-terminal">
      <input name="command" placeholder="Run in workspace, e.g. git status --short" ${workspace ? "" : "disabled"} />
      <button type="submit" ${workspace ? "" : "disabled"}>Run</button>
    </form>
    <div class="pty-panel">
      <div class="pty-toolbar">
        <button data-action-click="start-pty" ${workspace ? "" : "disabled"}>New terminal</button>
        <button data-action-click="stop-pty" ${terminal?.isRunning ? "" : "disabled"}>Stop</button>
      </div>
      <pre class="pty-output">${escapeHtml(terminal?.output ?? "No terminal session")}</pre>
      <form class="pty-input" data-action="pty-send">
        <input name="input" placeholder="Type shell input" ${terminal?.isRunning ? "" : "disabled"} />
        <button type="submit" ${terminal?.isRunning ? "" : "disabled"}>Send</button>
      </form>
    </div>
  `;
}

function currentPtyTerminal() {
  if (!ptyTerminals.length) return null;
  return ptyTerminals.find((terminal) => terminal.id === selectedTerminalId) ?? ptyTerminals[0];
}

function renderCommandPalette() {
  if (!commandPaletteOpen) return "";
  const slashCommands = workspaceInitInfo?.slashCommands ?? [];
  const commands = [
    ["new-claude", "New Claude chat", "Start a Claude Code session"],
    ["new-codex", "New Codex chat", "Start a Codex session"],
    ["checkpoint", "Save checkpoint", "Write refs/loomen-checkpoints"],
    ["diff", "Show diff", "Compare workspace against checkpoint"],
    ["run-setup", "Run setup", "Execute repository setup script"],
    ["run-script", "Run script", "Execute repository run script"],
    ["open-workspace", "Open workspace", "Open this workspace in Finder"],
    ["open-repo", "Open repository", "Open the root repository in Finder"],
    ["repo-settings", "Repo settings", "Open repository details and branch info"],
    ["launch-health", "Launch health", "Ray local runtime dependencies"],
    ["settings", "Settings", "Open Loomen-style settings"]
  ];
  const query = paletteQuery.trim().toLowerCase();
  const commandRows = commands.filter(([id, title, detail]) =>
    [id, title, detail].some((value) => value.toLowerCase().includes(query))
  );
  const slashRows = slashCommands.filter((command) =>
    [command.name || "", command.path || ""].some((value) => value.toLowerCase().includes(query))
  );
  const fileRows = query
    ? files
        .filter((file) => file.path.toLowerCase().includes(query.replace(/^@/, "")))
        .slice(0, 12)
    : [];
  const visibleSlashRows = slashRows.slice(0, 12);
  const visibleCount = commandRows.length + visibleSlashRows.length + fileRows.length;
  if (paletteSelectedIndex >= visibleCount) paletteSelectedIndex = Math.max(0, visibleCount - 1);
  let rowIndex = 0;
  return `
    <div class="palette-backdrop">
      <section class="command-palette">
        <input data-action="palette-search" value="${escapeAttr(paletteQuery)}" placeholder="Search commands, /commands, files" autofocus />
        <div class="palette-list">
          ${commandRows.map(([id, title, detail]) => renderPaletteButton(rowIndex++, id, title, detail)).join("")}
          ${visibleSlashRows.length ? `<div class="palette-heading">Slash commands</div>` : ""}
          ${visibleSlashRows.map((command) => renderPaletteButton(rowIndex++, `slash:${command.name}`, command.name, command.path || "workspace command")).join("")}
          ${fileRows.length ? `<div class="palette-heading">Files</div>` : ""}
          ${fileRows.map((file) => renderPaletteButton(rowIndex++, `file:${file.path}`, file.path, `Open ${file.kind}`)).join("")}
          ${!visibleCount ? `<div class="palette-empty">No commands</div>` : ""}
        </div>
      </section>
    </div>
  `;
}

function renderPaletteButton(index, action, title, detail) {
  return `
    <button class="${index === paletteSelectedIndex ? "active" : ""}" data-command-index="${index}" data-command-action="${escapeAttr(action)}">
      <strong>${escapeHtml(title)}</strong>
      <span>${escapeHtml(detail)}</span>
    </button>
  `;
}

function movePaletteSelection(delta) {
  const items = [...document.querySelectorAll("[data-command-index]")];
  if (!items.length) return;
  items[paletteSelectedIndex]?.classList.remove("active");
  paletteSelectedIndex = (paletteSelectedIndex + delta + items.length) % items.length;
  items[paletteSelectedIndex]?.classList.add("active");
  items[paletteSelectedIndex]?.scrollIntoView({ block: "nearest" });
}

async function runCommandAction(action) {
  commandPaletteOpen = false;
  paletteQuery = "";
  paletteSelectedIndex = 0;
  if (!action) return render();
  if (action.startsWith("slash:")) {
    draftPrompt = `${action.slice("slash:".length)} `;
    render();
    document.querySelector('[data-action="send-query"] textarea[name="prompt"]')?.focus();
    return;
  }
  if (action.startsWith("file:")) {
    activeRightPanel = "files";
    await openWorkspaceFile(action.slice("file:".length));
    return;
  }
  if (action === "new-claude") return newSession("claude");
  if (action === "new-codex") return newSession("codex");
  if (action === "checkpoint") return saveCheckpoint();
  if (action === "diff") return showDiff();
  if (action === "run-setup") return runWorkspaceSetup();
  if (action === "run-script") return runWorkspaceScript();
  if (action === "open-workspace") return openWorkspaceInFinder();
  if (action === "open-repo") return openRepoInFinder();
  if (action === "repo-settings") return openCurrentRepoSettings();
  if (action === "launch-health") {
    settingsTab = "health";
    view = "settings";
    await refreshLaunchHealth();
    return;
  }
  if (action === "settings") {
    view = "settings";
    return render();
  }
  render();
}

function handleGlobalKeys(event) {
  const meta = event.metaKey || event.ctrlKey;
  if (event.key === "Escape" && commandPaletteOpen) {
    commandPaletteOpen = false;
    paletteQuery = "";
    paletteSelectedIndex = 0;
    render();
    return;
  }
  if (event.key === "Escape" && notificationsOpen) {
    notificationsOpen = false;
    render();
    return;
  }
  if (meta && event.key.toLowerCase() === "k") {
    event.preventDefault();
    paletteQuery = "";
    paletteSelectedIndex = 0;
    commandPaletteOpen = true;
    render();
    return;
  }
  if (event.altKey && event.key.toLowerCase() === "t") {
    event.preventDefault();
    notificationsOpen = !notificationsOpen;
    render();
    return;
  }
  if (meta && event.key.toLowerCase() === "l") {
    event.preventDefault();
    document.querySelector('[data-action="send-query"] textarea[name="prompt"]')?.focus();
    return;
  }
  if (meta && event.key === ",") {
    event.preventDefault();
    view = "settings";
    render();
    return;
  }
  if (meta && event.shiftKey && event.key.toLowerCase() === "t") {
    event.preventDefault();
    activeRunTab = "terminal";
    void startPtyTerminal();
  }
}

function openCurrentRepoSettings() {
  const repo = currentRepo();
  if (!repo) return render();
  settingsTab = `repo:${repo.id}`;
  view = "settings";
  render();
}

async function openWorkspaceInFinder() {
  const workspace = currentWorkspace();
  const repo = currentRepo();
  lastError = "";
  if (workspace?.id) {
    await invoke("open_workspace_in_finder", { workspaceId: workspace.id }).catch((error) => {
      lastError = String(error);
      pushNotification("Open workspace failed", lastError, "error");
    });
  } else if (repo?.id) {
    await invoke("open_repo_in_finder", { repoId: repo.id }).catch((error) => {
      lastError = String(error);
      pushNotification("Open repo failed", lastError, "error");
    });
  }
  render();
}

async function openRepoInFinder() {
  const repo = currentRepo();
  if (!repo?.id) return;
  lastError = "";
  await invoke("open_repo_in_finder", { repoId: repo.id }).catch((error) => {
    lastError = String(error);
    pushNotification("Open repo failed", lastError, "error");
  });
  render();
}

function renderSettings() {
  const repoItems = snapshot.repos
    .map((repo) => `<button class="settings-nav-item repo ${settingsTab === `repo:${repo.id}` ? "active" : ""}" data-settings-repo="${escapeAttr(repo.id)}">${escapeHtml(repo.name)}<span>${escapeHtml(repo.path)}</span></button>`)
    .join("");
  return `
    <main class="settings-shell">
      <aside class="settings-sidebar">
        <button class="back-button" data-action-click="back-to-app">← Back to app</button>
        <button class="${settingsTab === "general" ? "active" : ""}" data-settings-tab="general">General</button>
        <button class="${settingsTab === "models" ? "active" : ""}" data-settings-tab="models">Models</button>
        <button class="${settingsTab === "providers" ? "active" : ""}" data-settings-tab="providers">Providers</button>
        <button class="${settingsTab === "appearance" ? "active" : ""}" data-settings-tab="appearance">Appearance</button>
        <button class="${settingsTab === "git" ? "active" : ""}" data-settings-tab="git">Git</button>
        <button class="${settingsTab === "account" ? "active" : ""}" data-settings-tab="account">Account</button>
        <div class="settings-heading">More</div>
        <button class="${settingsTab === "health" ? "active" : ""}" data-settings-tab="health">Health</button>
        <button class="${settingsTab === "experimental" ? "active" : ""}" data-settings-tab="experimental">Experimental</button>
        <button class="${settingsTab === "advanced" ? "active" : ""}" data-settings-tab="advanced">Advanced</button>
        <div class="settings-heading">Repositories</div>
        ${repoItems || `<span class="settings-muted">No repositories</span>`}
      </aside>
      <form class="settings-content" data-action="settings-form">
        ${renderSettingsPanel(settingsTab)}
        <footer class="settings-footer">
          <span>${lastError ? escapeHtml(lastError) : "Settings persist to the local SQLite settings table."}</span>
          <button type="submit">Save settings</button>
        </footer>
      </form>
    </main>
  `;
}

function renderSettingsPanel(tab) {
  const s = settings || {};
  if (tab?.startsWith("repo:")) {
    return renderRepoSettingsPanel(tab.slice("repo:".length));
  }
  switch (tab) {
    case "models":
      return `
        <h1>Models</h1>
        ${settingsSelect("defaultClaudeModel", "Default Claude model", "Model for new Claude chats", ["opus", "sonnet", "haiku"], s.defaultClaudeModel)}
        ${settingsSelect("defaultCodexModel", "Default Codex model", "Model for new Codex chats", ["gpt-5-codex", "gpt-5", "gpt-5.2"], s.defaultCodexModel)}
        ${settingsSelect("defaultCodexEffort", "Codex effort", "Thinking level for new Codex chats", ["high", "medium", "low", "xhigh"], s.defaultCodexEffort)}
        ${settingsSelect("codexPersonality", "Codex personality for new chats", "Style to use when a new chat starts with a Codex model", ["Default", "Concise", "Reviewer", "Planner"], s.codexPersonality)}
        ${settingsSelect("reviewModel", "Review model", "Model for code reviews", ["opus", "sonnet", "gpt-5-codex"], s.reviewModel)}
        ${settingsSelect("reviewCodexEffort", "Review effort", "Thinking level for review sessions", ["high", "medium", "low", "xhigh"], s.reviewCodexEffort)}
        ${settingsSwitch("defaultToPlanMode", "Default to plan mode", "Start new chats in plan mode", s.defaultToPlanMode)}
        ${settingsSwitch("defaultToFastMode", "Default to fast mode", "Start new chats in fast mode", s.defaultToFastMode)}
        ${settingsSwitch("claudeChrome", "Use Claude Code with Chrome", "Allow Claude Code browser-control features when available.", s.claudeChrome)}
      `;
    case "providers":
      return `
        <h1>Providers</h1>
        <p class="settings-intro">Configure API keys and environment variables for Claude Code and Codex. Values are stored locally in this rebuild.</p>
        <h2>Claude Code</h2>
        ${settingsTextarea("providerEnv", "Environment variables", "One per line, e.g. VAR_NAME=value or export VAR_NAME=value", s.providerEnv, "ANTHROPIC_API_KEY=...\nCLAUDE_CODE_USE_BEDROCK=1\nAWS_REGION=us-east-1")}
        <h2>Codex</h2>
        <div class="provider-cards">
          <label class="${s.codexProviderMode === "cli" ? "active" : ""}"><input type="radio" name="codexProviderMode" value="cli" ${s.codexProviderMode === "cli" ? "checked" : ""}/> CLI</label>
          <label class="${s.codexProviderMode === "apiKey" ? "active" : ""}"><input type="radio" name="codexProviderMode" value="apiKey" ${s.codexProviderMode === "apiKey" ? "checked" : ""}/> API Key</label>
        </div>
        <div class="settings-note">CLI mode uses your local Codex login. API-key mode feeds configured env vars into the child process.</div>
      `;
    case "appearance":
      return `
        <h1>Appearance</h1>
        ${settingsSelect("theme", "Theme", "Toggle with keyboard shortcuts in the real app", ["Dark", "Light", "System"], s.theme)}
        ${settingsSwitch("coloredSidebarDiffs", "Colored sidebar diffs", "Always show line change colors in the sidebar.", s.coloredSidebarDiffs)}
        ${settingsSelect("monoFont", "Mono Font", "Font used for code and diffs", ["Geist Mono", "SF Mono", "Menlo", "JetBrains Mono"], s.monoFont)}
        <pre class="font-preview">// Preview
const greeting = 'Hello, World!';
function sum(a, b) { return a + b; }</pre>
        ${settingsSelect("markdownStyle", "Markdown Style", "Rendering style for markdown files", ["Default", "Dense", "Document"], s.markdownStyle)}
        ${settingsInput("terminalFont", "Terminal Font", "Enter font name exactly as installed", s.terminalFont, "Leave empty for default")}
        ${settingsInput("terminalFontSize", "Terminal Font Size", "px", s.terminalFontSize ?? 12, "12", "number")}
      `;
    case "git":
      return `
        <h1>Git</h1>
        ${settingsSelect("branchPrefixType", "Branch name prefix", "Prefix for new workspace branch names", ["github_username", "custom", "none"], s.branchPrefixType)}
        ${settingsInput("branchPrefixCustom", "Custom branch prefix", "Used when branch prefix type is Custom", s.branchPrefixCustom, "team-name")}
        ${settingsSwitch("deleteBranchOnArchive", "Delete branch on archive", "Delete the local branch when archiving a workspace.", s.deleteBranchOnArchive)}
        ${settingsSwitch("archiveOnMerge", "Archive on merge", "Automatically archive a workspace after merging its PR.", s.archiveOnMerge)}
      `;
    case "account":
      return `
        <h1>Account</h1>
        <section class="account-card">
          <div class="avatar">CR</div>
          <div><strong>Local rebuild</strong><span>No cloud account is required for this clean-room implementation.</span></div>
        </section>
        ${settingsSwitch("enterpriseDataPrivacy", "Enterprise data privacy", "Disable features that require external AI providers or cloud-only integrations.", s.enterpriseDataPrivacy)}
        ${settingsSwitch("claudeToolApprovals", "Claude Code tool approvals", "Require manual approval before agents can run tools.", s.claudeToolApprovals)}
      `;
    case "health":
      return renderLaunchHealthPanel();
    case "experimental":
      return `
        <h1>Experimental</h1>
        <p class="settings-intro">Experimental features under development. These mirror the installed app’s toggles as local state first.</p>
        ${settingsSwitch("bigTerminalMode", "Big terminal mode", "Create big terminals in the center pane.", s.bigTerminalMode)}
        ${settingsSwitch("dashboard", "Dashboard", "Show the dashboard and workspace sidebar.", s.dashboard)}
        ${settingsSwitch("voiceMode", "Voice mode", "Enable speech-to-text entry points when wired.", s.voiceMode)}
        ${settingsSwitch("automerge", "Automerge", "Show automerge actions when checks are pending.", s.automerge)}
        ${settingsSwitch("spotlightTesting", "Use spotlight testing", "Replace Run with a Spotlight testing path.", s.spotlightTesting)}
        ${settingsSwitch("sidebarResourceUsage", "Show sidebar resource usage", "Show CPU and memory usage in the workspace sidebar footer.", s.sidebarResourceUsage)}
        ${settingsSwitch("matchWorkspaceDirectoryWithBranchName", "Match workspace directory with branch name", "Rename workspace directories when branch names change.", s.matchWorkspaceDirectoryWithBranchName)}
        ${settingsSwitch("experimentalTerminalRuntime", "Use experimental terminal runtime", "Use the new runtime for interactive workspace terminals.", s.experimentalTerminalRuntime)}
        ${settingsSwitch("reactProfiler", "React profiler", "Show a record button for render traces.", s.reactProfiler)}
      `;
    case "advanced":
      return `
        <h1>Advanced</h1>
        ${settingsInput("loomenRootDirectory", "Loomen root directory", "Where repositories and workspaces are stored.", s.loomenRootDirectory, "~/loomen")}
        ${settingsInput("claudeExecutablePath", "Claude Code executable path", "Override the bundled Claude Code executable. Leave empty to use bundled/PATH.", s.claudeExecutablePath, "/usr/local/bin/claude")}
        ${settingsInput("codexExecutablePath", "Codex executable path", "Override the bundled Codex executable. Leave empty to use bundled/PATH.", s.codexExecutablePath, "/opt/homebrew/bin/codex")}
      `;
    default:
      return `
        <h1>General</h1>
        ${settingsSelect("sendMessagesWith", "Send messages with", "Choose which key combination sends messages", ["Enter", "Shift+Enter", "Cmd+Enter"], s.sendMessagesWith)}
        ${settingsSwitch("desktopNotifications", "Desktop notifications", "Get notified when AI finishes working in a chat.", s.desktopNotifications)}
        ${settingsSwitch("soundEffects", "Sound effects", "Play a sound when AI finishes working in a chat.", s.soundEffects)}
        ${settingsSwitch("autoConvertLongText", "Auto-convert long text", "Convert pasted text over 5000 characters into text attachments.", s.autoConvertLongText)}
        ${settingsSwitch("stripAbsoluteRight", "I'm not absolutely right, thank you very much", "Strip repetitive agreement phrases from AI messages.", s.stripAbsoluteRight)}
        ${settingsSwitch("alwaysShowContextUsage", "Always show context usage", "By default it is only shown when more than 70% is used.", s.alwaysShowContextUsage)}
        ${settingsSwitch("expandToolCalls", "Don't collapse tool calls", "Show all tool calls expanded by default.", s.expandToolCalls)}
      `;
  }
}

function renderLaunchHealthPanel() {
  const health = launchHealth || fallbackLaunchHealth("Launch health has not loaded yet.");
  const generated = health.generatedAt ? new Date(health.generatedAt).toLocaleTimeString() : "not yet";
  return `
    <h1>Launch Health</h1>
    <p class="settings-intro">Ray the local runtime before work begins: Git, Bun, agent CLIs, GitHub CLI, database path, rebuild root, and sidecar socket state.</p>
    <section class="health-summary ${escapeAttr(health.status || "warning")}">
      <div>
        <strong>${escapeHtml(healthStatusTitle(health.status))}</strong>
        <span>Last checked ${escapeHtml(generated)}</span>
      </div>
      <button type="button" data-action-click="refresh-launch-health">Refresh</button>
    </section>
    <section class="health-grid">
      ${(health.checks || []).map(renderHealthCheck).join("")}
    </section>
  `;
}

function renderHealthCheck(check) {
  const status = check.status || "warning";
  const meta = [
    check.required ? "required" : "optional",
    check.version,
    check.path
  ].filter(Boolean);
  return `
    <article class="health-check ${escapeAttr(status)}">
      <header>
        <span>${escapeHtml(status)}</span>
        <strong>${escapeHtml(check.label || check.id || "Check")}</strong>
      </header>
      <p>${escapeHtml(check.detail || "")}</p>
      ${meta.length ? `<code>${escapeHtml(meta.join(" · "))}</code>` : ""}
      ${check.remediation ? `<em>${escapeHtml(check.remediation)}</em>` : ""}
    </article>
  `;
}

function healthStatusTitle(status) {
  if (status === "ok") return "All required runtime checks are clear";
  if (status === "error") return "Runtime attention needed";
  return "Runtime mostly ready, with notes";
}

function renderRepoSettingsPanel(repoId) {
  const repo = snapshot.repos.find((item) => item.id === repoId);
  if (!repo) return `<h1>Repository</h1><p class="settings-intro">Repository not found.</p>`;
  return `
    <h1>${escapeHtml(repo.name)}</h1>
    <section class="repo-settings-card">
      <div><strong>Path</strong><span>${escapeHtml(repo.path)}</span></div>
      <div><strong>Current branch</strong><span>${escapeHtml(repo.currentBranch || "unknown")}</span></div>
      <div><strong>Default branch</strong><span>${escapeHtml(repo.defaultBranch || "unknown")}</span></div>
      <div><strong>Remote</strong><span>${escapeHtml(repo.remote || "none")}</span></div>
      <div><strong>Workspaces</strong><span>${escapeHtml(repo.workspaces.length)}</span></div>
    </section>
    <div class="repo-settings-actions">
      <button type="button" data-open-repo-path-settings="${escapeAttr(repo.id)}">Open in Finder</button>
      <button type="button" data-select-repo-from-settings="${escapeAttr(repo.id)}">View workbench</button>
    </div>
    <h2>Branches</h2>
    <div class="branch-list">
      ${(repo.branches || []).map((branch) => `<code>${escapeHtml(branch)}</code>`).join("") || `<span class="settings-muted">No branches found</span>`}
    </div>
    <h2>Workspaces</h2>
    <div class="repo-workspace-list">
      ${
        repo.workspaces
          .map(
            (workspace) => `
              <button type="button" data-select-workspace-from-settings="${escapeAttr(workspace.id)}" data-select-repo-from-settings="${escapeAttr(repo.id)}">
                <strong>${escapeHtml(workspace.name)}</strong>
                <span>${escapeHtml([workspace.branchName, workspace.state, workspace.path].filter(Boolean).join(" · "))}</span>
              </button>
            `
          )
          .join("") || `<span class="settings-muted">No workspaces</span>`
      }
    </div>
  `;
}

function bindSettingsEvents() {
  document.querySelector('[data-action-click="back-to-app"]')?.addEventListener("click", async () => {
    view = "workbench";
    snapshot = await invoke("get_state").catch(() => snapshot);
    selectFallbacks();
    await refreshWorkspacePanels();
    render();
  });
  document.querySelectorAll("[data-settings-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      settingsTab = button.dataset.settingsTab;
      render();
    });
  });
  document.querySelectorAll("[data-settings-repo]").forEach((button) => {
    button.addEventListener("click", () => {
      settingsTab = `repo:${button.dataset.settingsRepo}`;
      render();
    });
  });
  document.querySelectorAll("[data-open-repo-path-settings]").forEach((button) => {
    button.addEventListener("click", async () => {
      await invoke("open_repo_in_finder", { repoId: button.dataset.openRepoPathSettings }).catch((error) => {
        lastError = String(error);
      });
      render();
    });
  });
  document.querySelectorAll("[data-select-repo-from-settings]").forEach((button) => {
    button.addEventListener("click", async () => {
      selection.repoId = button.dataset.selectRepoFromSettings;
      selection.workspaceId = button.dataset.selectWorkspaceFromSettings || selection.workspaceId;
      view = "workbench";
      selectFallbacks();
      await refreshWorkspacePanels();
      render();
    });
  });
  document.querySelector('[data-action-click="refresh-launch-health"]')?.addEventListener("click", refreshLaunchHealth);
  document.querySelector('[data-action="settings-form"]')?.addEventListener("submit", saveSettings);
  document.querySelector('[data-action="settings-form"]')?.addEventListener("change", saveSettings);
}

async function saveSettings(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const next = { ...(settings || {}) };
  for (const element of form.elements) {
    if (!element.name) continue;
    if (element.type === "checkbox") {
      next[element.name] = element.checked;
    } else if (element.type === "radio") {
      if (element.checked) next[element.name] = element.value;
    } else if (element.type === "number") {
      next[element.name] = Number(element.value || 0);
    } else {
      next[element.name] = element.value;
    }
  }
  settings = await invoke("update_settings", { settings: next }).catch((error) => {
    lastError = String(error);
    return settings;
  });
  render();
}

function settingsInput(name, title, description, value, placeholder = "", type = "text") {
  return `
    <label class="setting-row">
      <span><strong>${escapeHtml(title)}</strong><em>${escapeHtml(description)}</em></span>
      <input name="${escapeAttr(name)}" type="${escapeAttr(type)}" value="${escapeAttr(value ?? "")}" placeholder="${escapeAttr(placeholder)}" />
    </label>
  `;
}

function settingsTextarea(name, title, description, value, placeholder = "") {
  return `
    <label class="setting-row vertical">
      <span><strong>${escapeHtml(title)}</strong><em>${escapeHtml(description)}</em></span>
      <textarea name="${escapeAttr(name)}" rows="5" placeholder="${escapeAttr(placeholder)}">${escapeHtml(value ?? "")}</textarea>
    </label>
  `;
}

function settingsSelect(name, title, description, options, current) {
  return `
    <label class="setting-row">
      <span><strong>${escapeHtml(title)}</strong><em>${escapeHtml(description)}</em></span>
      <select name="${escapeAttr(name)}">
        ${options.map((option) => `<option value="${escapeAttr(option)}" ${option === current ? "selected" : ""}>${escapeHtml(option)}</option>`).join("")}
      </select>
    </label>
  `;
}

function settingsSwitch(name, title, description, checked) {
  return `
    <label class="setting-row">
      <span><strong>${escapeHtml(title)}</strong><em>${escapeHtml(description)}</em></span>
      <input class="switch-input" name="${escapeAttr(name)}" type="checkbox" ${checked ? "checked" : ""} />
    </label>
  `;
}

function renderComposerSuggestions(session) {
  if (!session || !draftPrompt) return "";
  const token = currentComposerToken();
  if (!token || (!token.startsWith("/") && !token.startsWith("@"))) return "";

  const suggestions = token.startsWith("/")
    ? slashCommandSuggestions(token)
    : mentionSuggestions(token);
  if (!suggestions.length) return "";

  return `
    <div class="composer-suggestions">
      ${suggestions
        .slice(0, 10)
        .map(
          (item) => `
            <button type="button" data-composer-suggestion="${escapeAttr(item.insert)}">
              <strong>${escapeHtml(item.label)}</strong>
              <span>${escapeHtml(item.detail)}</span>
            </button>
          `
        )
        .join("")}
    </div>
  `;
}

function currentComposerToken() {
  const match = draftPrompt.match(/(^|\s)(\S*)$/);
  return match?.[2] ?? "";
}

function slashCommandSuggestions(token) {
  const query = token.slice(1).toLowerCase();
  const discovered = (workspaceInitInfo?.slashCommands ?? []).map((command) => ({
    label: command.name || "/command",
    insert: command.name || "/command",
    detail: command.path || "Claude command"
  }));
  const builtIns = [
    { label: "/review", insert: "/review", detail: "Review current workspace changes" },
    { label: "/status", insert: "/status", detail: "Show agent and tool status" },
    { label: "/compact", insert: "/compact", detail: "Compact the current conversation" },
    { label: "/clear", insert: "/clear", detail: "Start with a clean context" }
  ];
  return [...discovered, ...builtIns]
    .filter((item, index, all) => all.findIndex((other) => other.label === item.label) === index)
    .filter((item) => !query || item.label.toLowerCase().includes(query));
}

function mentionSuggestions(token) {
  const query = token.slice(1).toLowerCase();
  const note = { label: "@notes", insert: "@notes", detail: "Scratchpad notes for this workspace" };
  const fileItems = files.map((file) => ({
    label: `@${file.path}`,
    insert: `@${file.path}`,
    detail: file.kind
  }));
  return [note, ...fileItems].filter((item) => !query || item.label.toLowerCase().includes(query)).slice(0, 50);
}

function applyComposerSuggestion(value) {
  if (!value) return;
  const match = draftPrompt.match(/(^|\s)(\S*)$/);
  if (!match) {
    draftPrompt = `${value} `;
  } else {
    draftPrompt = `${draftPrompt.slice(0, draftPrompt.length - match[2].length)}${value} `;
  }
  render();
  const textarea = document.querySelector('[data-action="send-query"] textarea[name="prompt"]');
  if (textarea) {
    textarea.focus();
    textarea.selectionStart = textarea.selectionEnd = textarea.value.length;
  }
}

function updateComposerSuggestions() {
  const slot = document.querySelector(".composer-suggestions-slot");
  if (!slot) return;
  slot.innerHTML = renderComposerSuggestions(currentSession());
  bindComposerSuggestionEvents(slot);
}

function renderLiveMessage(session) {
  if (!session) return "";
  const chunks = liveMessages[session.id] ?? [];
  if (!chunks.length) return "";
  return `
    <article class="message assistant streaming">
      <strong>assistant · streaming</strong>
      <pre>${escapeHtml(chunks.join("\n"))}</pre>
    </article>
  `;
}

function renderLiveSessionEvents(session) {
  if (!session) return "";
  const events = liveSessionEvents[session.id] ?? [];
  if (!events.length) return "";
  return `
    <article class="message system session-events">
      <strong>tool activity</strong>
      <div>
        ${events.map(renderSessionEventCard).join("")}
      </div>
    </article>
  `;
}

function renderSessionEventCard(event) {
  const inner = unwrapSessionEvent(event);
  const tool = inner.message?.content?.find((item) => item.type && item.type !== "text");
  const title = summarizeSessionEvent(inner);
  const detail = tool?.input ?? tool?.content ?? inner.result ?? inner;
  return `
    <details class="tool-event">
      <summary>${escapeHtml(title)}</summary>
      <pre>${escapeHtml(JSON.stringify(detail, null, 2))}</pre>
    </details>
  `;
}

function unwrapSessionEvent(event) {
  if (event?.event && typeof event.event === "object") return event.event;
  return event || {};
}

function summarizeSessionEvent(event) {
  event = unwrapSessionEvent(event);
  const type = event.type || event.event || "event";
  if (event.message?.content?.length) {
    const tool = event.message.content.find((item) => item.type && item.type !== "text");
    if (tool) return `${type}: ${tool.type}${tool.name ? ` ${tool.name}` : ""}`;
  }
  if (event.toolName || event.name) return `${type}: ${event.toolName || event.name}`;
  return type;
}

function renderContextUsage() {
  if (!contextUsage) return "";
  return `
    <div class="context-usage">
      <div>
        <span>Context</span>
        <strong>${escapeHtml(formatNumber(contextUsage.usedTokens))} / ${escapeHtml(formatNumber(contextUsage.maxTokens))}</strong>
      </div>
      <meter min="0" max="100" value="${escapeAttr(contextUsage.percent ?? 0)}"></meter>
    </div>
  `;
}

function renderToolApprovalModal() {
  const approval = toolApprovals[0];
  if (!approval) return "";
  return `
    <div class="modal-backdrop">
      <section class="approval-modal">
        <header>
          <strong>Tool approval</strong>
          <span>${escapeHtml(approval.permissionMode || "default")}</span>
        </header>
        <div class="approval-body">
          <div>
            <span>Tool</span>
            <strong>${escapeHtml(approval.toolName || "tool")}</strong>
          </div>
          <pre>${escapeHtml(JSON.stringify(approval.input ?? {}, null, 2))}</pre>
        </div>
        <footer>
          <button data-approval-id="${escapeAttr(approval.approvalId)}" data-approval-decision="reject">Reject</button>
          <button class="primary" data-approval-id="${escapeAttr(approval.approvalId)}" data-approval-decision="approve">Approve</button>
        </footer>
      </section>
    </div>
  `;
}

function renderPrCreateModal() {
  if (!prModalOpen) return "";
  const workspace = currentWorkspace();
  const session = currentSession();
  const isEditing = prInfo && !prInfo.error && prInfo.number;
  const defaultTitle = isEditing
    ? prInfo.title || "Pull request"
    : session?.title && session.title !== "Untitled" ? session.title : workspace?.name || "Workspace changes";
  return `
    <div class="modal-backdrop">
      <form class="approval-modal pr-modal" data-action="create-pr">
        <header>
          <strong>${isEditing ? "Edit pull request" : "Publish pull request"}</strong>
          <span>${escapeHtml(workspace?.branchName || "workspace branch")}</span>
        </header>
        <div class="approval-body">
          <label>
            <span>Title</span>
            <input name="title" value="${escapeAttr(defaultTitle)}" required />
          </label>
          <label>
            <span>Body</span>
            <textarea name="body" rows="8" placeholder="Describe the changes">${escapeHtml(defaultPrBody(workspace, isEditing))}</textarea>
          </label>
          ${isEditing ? "" : `
            <label class="inline-check">
              <input name="draft" type="checkbox" checked />
              <span>Create as draft</span>
            </label>
          `}
          <p class="modal-note">This calls GitHub CLI from the workspace directory when you submit.</p>
        </div>
        <footer>
          <button type="button" data-action-click="close-pr-modal">Cancel</button>
          <button class="primary" type="submit">${isEditing ? "Update PR" : "Publish PR"}</button>
        </footer>
      </form>
    </div>
  `;
}

function defaultPrBody(workspace, isEditing = false) {
  if (isEditing) return "Updated from Loomen.";
  const changeSummary = changes.length
    ? changes.slice(0, 12).map((item) => `- ${item.kind} ${item.path}`).join("\n")
    : "- No local file changes detected yet.";
  return `## Summary
${changeSummary}

## Test plan
- Not run yet.`;
}

async function createPullRequest(event) {
  event.preventDefault();
  const workspace = currentWorkspace();
  if (!workspace) return;
  const form = new FormData(event.currentTarget);
  const payload = {
    workspaceId: workspace.id,
    title: form.get("title")?.toString() ?? "",
    body: form.get("body")?.toString() ?? ""
  };
  prInfo = prInfo && !prInfo.error && prInfo.number
    ? await invoke("update_pull_request", payload).catch((error) => ({ error: String(error), checks: [] }))
    : await invoke("create_pull_request", { ...payload, draft: form.get("draft") === "on" }).catch((error) => ({ error: String(error), checks: [] }));
  prModalOpen = false;
  activeRightPanel = "checks";
  render();
}

async function refreshPullRequestInfo() {
  const workspace = currentWorkspace();
  if (!workspace) return;
  prInfo = await invoke("get_pull_request_info", { workspaceId: workspace.id }).catch((error) => ({
    error: String(error),
    checks: []
  }));
  render();
}

async function rerunFailedChecks() {
  const workspace = currentWorkspace();
  if (!workspace) return;
  lastError = "";
  const message = await invoke("rerun_failed_checks", { workspaceId: workspace.id }).catch((error) => {
    lastError = String(error);
    return "";
  });
  if (message) lastError = message;
  await refreshPullRequestInfo();
}

async function resolveToolApproval(decision, approvalId) {
  if (!approvalId) return;
  const approved = decision === "approve";
  await invoke("resolve_tool_approval", { approvalId, approved }).catch((error) => {
    lastError = String(error);
  });
  toolApprovals = toolApprovals.filter((approval) => approval.approvalId !== approvalId);
  render();
}

async function cancelCurrentQuery() {
  if (!pendingSessionId) return;
  await invoke("cancel_query", { sessionId: pendingSessionId }).catch((error) => {
    lastError = String(error);
  });
}

async function saveSessionSettings(event) {
  const session = currentSession();
  if (!session) return;
  const controls = event.currentTarget;
  snapshot = await invoke("update_session_settings", {
    sessionId: session.id,
    model: controls.querySelector('[name="model"]')?.value.trim() || defaultModelFor(session.agentType),
    permissionMode: controls.querySelector('[name="permissionMode"]')?.value || "default"
  });
  selectFallbacks();
  render();
}

function shouldSubmitComposer(event) {
  if (pending || !currentSession()) return false;
  const mode = settings?.sendMessagesWith || "Enter";
  if (event.key !== "Enter") return false;
  if (mode === "Cmd+Enter") return event.metaKey || event.ctrlKey;
  if (mode === "Shift+Enter") return event.shiftKey && !event.metaKey && !event.ctrlKey;
  return !event.shiftKey && !event.metaKey && !event.ctrlKey;
}

async function submitPromptForm(form) {
  const session = currentSession();
  if (!session || !form || pending) return;
  const prompt = new FormData(form).get("prompt")?.toString().trim();
  if (!prompt) return;
  pending = true;
  pendingSessionId = session.id;
  lastError = "";
  activeMainTab = "chat";
  render();
  try {
    snapshot = await invoke("start_query", { sessionId: session.id, prompt });
    draftPrompt = "";
    selectFallbacks();
    await refreshWorkspacePanels();
  } catch (error) {
    lastError = String(error);
    pushNotification("Failed to start agent", lastError, "error");
    pending = false;
    pendingSessionId = null;
  }
  render();
}

async function newSession(agentType) {
  const workspace = currentWorkspace();
  if (!workspace) return;
  snapshot = await invoke("create_session", { workspaceId: workspace.id, agentType });
  selection.workspaceId = workspace.id;
  selection.sessionId = snapshot.repos
    .flatMap((repo) => repo.workspaces)
    .find((item) => item.id === workspace.id)
    ?.sessions[0]?.id;
  activeMainTab = "chat";
  selectFallbacks();
  await refreshWorkspacePanels();
  render();
}

async function closeSession(sessionId) {
  if (!sessionId) return;
  snapshot = await invoke("close_session", { sessionId });
  if (selection.sessionId === sessionId) selection.sessionId = undefined;
  selectFallbacks();
  await refreshWorkspacePanels();
  render();
}

async function saveCheckpoint() {
  const workspace = currentWorkspace();
  if (!workspace) return;
  snapshot = await invoke("save_workspace_checkpoint", { workspaceId: workspace.id });
  selectFallbacks();
  await refreshWorkspacePanels();
  render();
}

async function showDiff() {
  const workspace = currentWorkspace();
  if (!workspace) return;
  const result = await invoke("get_workspace_diff", { workspaceId: workspace.id });
  lastDiff = result.diff;
  activeRightPanel = "changes";
  await refreshWorkspacePanels();
  render();
}

async function saveNotes(event) {
  const workspace = currentWorkspace();
  if (!workspace) return;
  snapshot = await invoke("update_workspace_notes", {
    workspaceId: workspace.id,
    notes: event.currentTarget.value
  });
  selectFallbacks();
  render();
}

async function refreshWorkspacePanels() {
  const workspace = currentWorkspace();
  if (!workspace) {
    files = [];
    fileFilter = "";
    filePreview = null;
    selectedPreviewLine = 0;
    workspaceSearchQuery = "";
    workspaceSearchResults = [];
    workspaceSearchPending = false;
    changes = [];
    diffFiles = [];
    diffComments = [];
    selectedDiffPath = "";
    changeFilter = "";
    prInfo = null;
    workspaceInitInfo = null;
    ptyTerminals = [];
    selectedTerminalId = "";
    contextUsage = null;
    return;
  }
  const session = currentSession();
  [files, changes, diffFiles, diffComments, prInfo, contextUsage] = await Promise.all([
    invoke("list_workspace_files", { workspaceId: workspace.id }).catch(() => []),
    invoke("list_workspace_changes", { workspaceId: workspace.id }).catch(() => []),
    invoke("get_workspace_patch", { workspaceId: workspace.id }).catch(() => []),
    invoke("list_diff_comments", { workspaceId: workspace.id }).catch(() => []),
    invoke("get_pull_request_info", { workspaceId: workspace.id }).catch((error) => ({ error: String(error), checks: [] })),
    session ? invoke("get_context_usage", { sessionId: session.id }).catch(() => null) : Promise.resolve(null)
  ]);
  workspaceInitInfo = await invoke("workspace_init", { workspaceId: workspace.id }).catch(() => null);
  ptyTerminals = await invoke("list_pty_terminals", { workspaceId: workspace.id }).catch(() => []);
  if (selectedTerminalId && !ptyTerminals.some((terminal) => terminal.id === selectedTerminalId)) {
    selectedTerminalId = ptyTerminals[0]?.id ?? "";
  } else if (!selectedTerminalId) {
    selectedTerminalId = ptyTerminals[0]?.id ?? "";
  }
  if (!selectedDiffPath || !diffFiles.some((file) => file.path === selectedDiffPath)) {
    selectedDiffPath = diffFiles[0]?.path ?? "";
  }
  if (filePreview && !files.some((file) => file.path === filePreview.path)) {
    filePreview = null;
  }
  spotlighter = await invoke("spotlighter_status", { workspaceId: workspace.id }).catch(() => null);
}

async function saveRepoScripts(event) {
  event.preventDefault();
  const repo = currentRepo();
  if (!repo) return;
  const form = new FormData(event.currentTarget);
  snapshot = await invoke("update_repo_scripts", {
    repoId: repo.id,
    setupScript: form.has("setupScript") ? form.get("setupScript")?.toString() ?? "" : repo.setupScript ?? "",
    runScript: form.has("runScript") ? form.get("runScript")?.toString() ?? "" : repo.runScript ?? "",
    runScriptMode: "concurrent"
  });
  setupScriptEditorOpen = false;
  runScriptEditorOpen = false;
  selectFallbacks();
  render();
}

async function runWorkspaceSetup() {
  const workspace = currentWorkspace();
  if (!workspace) return;
  lifecycleRun = await invoke("run_workspace_setup", { workspaceId: workspace.id });
  pushNotification("Setup finished", `exit ${lifecycleRun.exitCode ?? "signal"}`, lifecycleRun.exitCode === 0 ? "success" : "error");
  snapshot = await invoke("get_state");
  selectFallbacks();
  await refreshWorkspacePanels();
  render();
}

async function runWorkspaceScript() {
  const workspace = currentWorkspace();
  if (!workspace) return;
  lifecycleRun = await invoke("run_workspace_run_script", { workspaceId: workspace.id });
  pushNotification("Run script finished", `exit ${lifecycleRun.exitCode ?? "signal"}`, lifecycleRun.exitCode === 0 ? "success" : "error");
  await refreshWorkspacePanels();
  render();
}

async function archiveCurrentWorkspace() {
  const workspace = currentWorkspace();
  if (!workspace) return;
  snapshot = await invoke("archive_workspace", { workspaceId: workspace.id });
  selectFallbacks();
  await refreshWorkspacePanels();
  render();
}

async function restoreCurrentWorkspace() {
  const workspace = currentWorkspace();
  if (!workspace) return;
  snapshot = await invoke("restore_workspace", { workspaceId: workspace.id });
  selectFallbacks();
  await refreshWorkspacePanels();
  render();
}

async function startSpotlight() {
  const workspace = currentWorkspace();
  if (!workspace) return;
  spotlighter = await invoke("start_spotlighter", { workspaceId: workspace.id });
  render();
}

async function stopSpotlight() {
  const workspace = currentWorkspace();
  if (!workspace) return;
  spotlighter = await invoke("stop_spotlighter", { workspaceId: workspace.id }).catch(() => null);
  render();
}

async function startPtyTerminal() {
  const workspace = currentWorkspace();
  if (!workspace) return;
  const terminal = await invoke("start_pty_terminal", { workspaceId: workspace.id });
  selectedTerminalId = terminal.id;
  ptyTerminals = await invoke("list_pty_terminals", { workspaceId: workspace.id }).catch(() => [terminal]);
  activeRunTab = "terminal";
  startPtyPolling();
  render();
}

function startPtyPolling() {
  if (ptyPoller) clearInterval(ptyPoller);
  ptyPoller = setInterval(async () => {
    const workspace = currentWorkspace();
    if (!workspace) return;
    ptyTerminals = await invoke("list_pty_terminals", { workspaceId: workspace.id }).catch(() => ptyTerminals);
    if (selectedTerminalId && !ptyTerminals.some((terminal) => terminal.id === selectedTerminalId)) {
      selectedTerminalId = ptyTerminals[0]?.id ?? "";
    }
    render();
    if (!ptyTerminals.some((terminal) => terminal.isRunning) && ptyPoller) {
      clearInterval(ptyPoller);
      ptyPoller = null;
    }
  }, 1200);
}

async function sendPtyInput(event) {
  event.preventDefault();
  const terminal = currentPtyTerminal();
  if (!terminal?.id) return;
  const input = new FormData(event.currentTarget).get("input")?.toString() ?? "";
  if (!input) return;
  const updated = await invoke("write_pty_terminal", {
    terminalId: terminal.id,
    input: `${input}\n`
  });
  ptyTerminals = ptyTerminals.map((item) => (item.id === updated.id ? updated : item));
  render();
}

async function stopPtyTerminal() {
  const terminal = currentPtyTerminal();
  if (!terminal?.id) return;
  const updated = await invoke("stop_pty_terminal", { terminalId: terminal.id });
  ptyTerminals = ptyTerminals.map((item) => (item.id === updated.id ? updated : item));
  render();
}

async function closePtyTerminal(terminalId) {
  const workspace = currentWorkspace();
  if (!workspace || !terminalId) return;
  await invoke("close_pty_terminal", { terminalId }).catch((error) => {
    lastError = String(error);
  });
  ptyTerminals = await invoke("list_pty_terminals", { workspaceId: workspace.id }).catch(() =>
    ptyTerminals.filter((terminal) => terminal.id !== terminalId)
  );
  selectedTerminalId = ptyTerminals[0]?.id ?? "";
  render();
}

async function openWorkspaceFile(filePath, lineNumber = 0) {
  const workspace = currentWorkspace();
  if (!workspace || !filePath) return;
  selectedPreviewLine = Number(lineNumber || 0);
  filePreview = await invoke("read_workspace_file", {
    workspaceId: workspace.id,
    filePath
  }).catch((error) => ({
    workspaceId: workspace.id,
    path: filePath,
    content: String(error),
    isBinary: false,
    truncated: false
  }));
  render();
  if (selectedPreviewLine) {
    requestAnimationFrame(() => {
      document.querySelector(".preview-line.active")?.scrollIntoView({ block: "center" });
    });
  }
}

async function revealPreviewFile() {
  const workspace = currentWorkspace();
  if (!workspace || !filePreview?.path) return;
  lastError = "";
  await invoke("reveal_workspace_file", {
    workspaceId: workspace.id,
    filePath: filePreview.path
  }).catch((error) => {
    lastError = String(error);
    pushNotification("Reveal file failed", lastError, "error");
  });
  render();
}

async function openPreviewFileExternal() {
  const workspace = currentWorkspace();
  if (!workspace || !filePreview?.path) return;
  lastError = "";
  await invoke("open_workspace_file_external", {
    workspaceId: workspace.id,
    filePath: filePreview.path
  }).catch((error) => {
    lastError = String(error);
    pushNotification("Open file failed", lastError, "error");
  });
  render();
}

async function copyPreviewPath() {
  if (!filePreview?.path) return;
  if (!navigator.clipboard?.writeText) {
    lastError = "Clipboard API is unavailable";
    pushNotification("Copy path failed", lastError, "error");
    return render();
  }
  await navigator.clipboard.writeText(filePreview.path).then(
    () => pushNotification("Copied file path", filePreview.path, "success"),
    (error) => {
      lastError = String(error);
      pushNotification("Copy path failed", lastError, "error");
    }
  );
  render();
}

function currentSelectedDiffFile() {
  return diffFiles.find((file) => file.path === selectedDiffPath) ?? diffFiles[0] ?? null;
}

async function openSelectedDiffFile() {
  const selected = currentSelectedDiffFile();
  if (!selected?.path) return;
  activeRightPanel = "files";
  await openWorkspaceFile(selected.path, selectedCommentLine || 0);
}

async function revealSelectedDiffFile() {
  const workspace = currentWorkspace();
  const selected = currentSelectedDiffFile();
  if (!workspace || !selected?.path) return;
  lastError = "";
  await invoke("reveal_workspace_file", {
    workspaceId: workspace.id,
    filePath: selected.path
  }).catch((error) => {
    lastError = String(error);
    pushNotification("Reveal changed file failed", lastError, "error");
  });
  render();
}

async function copySelectedPatch() {
  const selected = currentSelectedDiffFile();
  if (!selected?.patch) return;
  if (!navigator.clipboard?.writeText) {
    lastError = "Clipboard API is unavailable";
    pushNotification("Copy patch failed", lastError, "error");
    return render();
  }
  await navigator.clipboard.writeText(selected.patch).then(
    () => pushNotification("Copied patch", selected.path, "success"),
    (error) => {
      lastError = String(error);
      pushNotification("Copy patch failed", lastError, "error");
    }
  );
  render();
}

async function searchWorkspaceContents(event) {
  event.preventDefault();
  const workspace = currentWorkspace();
  if (!workspace) return;
  const query = new FormData(event.currentTarget).get("query")?.toString().trim() || "";
  workspaceSearchQuery = query;
  workspaceSearchResults = [];
  if (!query) return render();
  workspaceSearchPending = true;
  render();
  workspaceSearchResults = await invoke("search_workspace", {
    workspaceId: workspace.id,
    query
  }).catch((error) => {
    lastError = String(error);
    pushNotification("Workspace search failed", lastError, "error");
    return [];
  });
  workspaceSearchPending = false;
  render();
}

function renderRightPanel() {
  if (activeRightPanel === "checks") {
    return renderChecksPanel();
  }
  if (activeRightPanel === "changes") {
    return renderChangesPanel();
  }
  return renderFilesPanel();
}

function renderFilesPanel() {
  if (!files.length) {
    return `<div class="panel-empty">No files</div>`;
  }
  const visibleFiles = fileFilter.trim()
    ? files.filter((file) => file.path.toLowerCase().includes(fileFilter.trim().toLowerCase()))
    : files;
  return `
    <div class="files-panel">
      <div class="file-filter">
        <input data-action="file-filter" value="${escapeAttr(fileFilter)}" placeholder="Filter files" />
        <span>${escapeHtml(visibleFiles.length)} / ${escapeHtml(files.length)}</span>
      </div>
      <form class="workspace-search" data-action="workspace-search">
        <input name="query" value="${escapeAttr(workspaceSearchQuery)}" placeholder="Search contents" />
        <button type="submit" ${workspaceSearchPending ? "disabled" : ""}>${workspaceSearchPending ? "Searching" : "Search"}</button>
      </form>
      ${renderWorkspaceSearchResults()}
      <div class="files-list file-tree">
        ${visibleFiles.length ? renderFileTree(visibleFiles) : `<div class="panel-empty small">No matching files</div>`}
      </div>
      <div class="file-preview">
        ${
          filePreview
            ? `
              <div class="file-preview-title">
                <strong>${escapeHtml(filePreview.path)}</strong>
                <span>${filePreview.truncated ? "truncated" : filePreview.isBinary ? "binary" : "preview"}</span>
                <div>
                  <button type="button" data-action-click="open-preview-file">Open</button>
                  <button type="button" data-action-click="copy-preview-path">Copy path</button>
                  <button type="button" data-action-click="reveal-preview-file">Reveal</button>
                </div>
              </div>
              ${renderFilePreviewContent(filePreview)}
            `
            : `<div class="panel-empty small">Select a file to preview</div>`
        }
      </div>
    </div>
  `;
}

function renderWorkspaceSearchResults() {
  if (!workspaceSearchQuery && !workspaceSearchResults.length) return "";
  if (!workspaceSearchResults.length) {
    return `<div class="search-results empty-results">${workspaceSearchPending ? "Searching..." : "No matches"}</div>`;
  }
  return `
    <div class="search-results">
      ${workspaceSearchResults
        .slice(0, 30)
        .map(
          (match) => `
            <button type="button" data-search-result-path="${escapeAttr(match.path)}" data-search-result-line="${escapeAttr(match.line)}">
              <strong>${escapeHtml(match.path)}:${escapeHtml(match.line)}</strong>
              <span>${escapeHtml(match.text.trim() || `column ${match.column}`)}</span>
            </button>
          `
        )
        .join("")}
    </div>
  `;
}

function renderFilePreviewContent(preview) {
  if (preview.isBinary) {
    return `<pre>${escapeHtml(preview.content)}</pre>`;
  }
  const lines = preview.content.split("\n");
  return `
    <div class="file-preview-code">
      ${lines
        .map((line, index) => {
          const lineNumber = index + 1;
          const active = selectedPreviewLine === lineNumber ? "active" : "";
          return `
            <div class="preview-line ${active}" data-preview-line="${lineNumber}">
              <span class="preview-line-no">${lineNumber}</span>
              <code>${escapeHtml(line || " ")}</code>
            </div>
          `;
        })
        .join("")}
    </div>
  `;
}

function renderFileTree(entries) {
  const root = { dirs: new Map(), files: [] };
  for (const entry of entries) {
    const parts = entry.path.split("/").filter(Boolean);
    let node = root;
    let currentPath = "";
    parts.forEach((part, index) => {
      const isFile = index === parts.length - 1;
      if (isFile) {
        node.files.push(entry);
        return;
      }
      currentPath = currentPath ? `${currentPath}/${part}` : part;
      if (!node.dirs.has(part)) {
        node.dirs.set(part, { name: part, path: currentPath, dirs: new Map(), files: [] });
      }
      node = node.dirs.get(part);
    });
  }
  return renderTreeNodeChildren(root, 0);
}

function renderTreeNodeChildren(node, depth) {
  const dirs = [...node.dirs.values()].sort((a, b) => a.name.localeCompare(b.name));
  const leafFiles = [...node.files].sort((a, b) => a.name.localeCompare(b.name));
  return [
    ...dirs.map((dir) => renderTreeDir(dir, depth)),
    ...leafFiles.map((file) => renderTreeFile(file, depth))
  ].join("");
}

function renderTreeDir(dir, depth) {
  const collapsed = collapsedFileDirs.has(dir.path);
  return `
    <div class="tree-branch">
      <button type="button" class="file-row tree-row dir-row" data-toggle-dir="${escapeAttr(dir.path)}" style="--depth:${depth}">
        <span class="tree-twist">${collapsed ? ">" : "v"}</span>
        <span class="file-kind">dir</span>
        <span>${escapeHtml(dir.name)}</span>
      </button>
      ${collapsed ? "" : renderTreeNodeChildren(dir, depth + 1)}
    </div>
  `;
}

function renderTreeFile(file, depth) {
  return `
    <button class="file-row tree-row ${filePreview?.path === file.path ? "active" : ""}" data-open-file="${escapeAttr(file.path)}" style="--depth:${depth}">
      <span class="tree-twist"></span>
      <span class="file-kind">${escapeHtml(fileIcon(file.kind))}</span>
      <span>${escapeHtml(file.name || file.path)}</span>
    </button>
  `;
}

function fileIcon(kind) {
  if (kind === "markdown") return "md";
  if (kind === "json") return "{}";
  if (kind === "lock") return "lock";
  if (kind === "git") return "git";
  return "file";
}

function renderPrSummary() {
  const workspace = currentWorkspace();
  if (!prInfo) {
    return `
      <div class="pr-summary loading">
        <strong>Loading PR info...</strong>
        <span>${escapeHtml(workspace?.branchName || "workspace branch")}</span>
      </div>
    `;
  }
  if (prInfo.error) {
    return `
      <div class="pr-summary">
        <strong>No pull request</strong>
        <span>${escapeHtml(prInfo.error || "No PR for this branch")}</span>
        <button data-action-click="open-pr-modal">Publish PR</button>
      </div>
    `;
  }
  return `
    <div class="pr-summary">
      <strong>#${escapeHtml(prInfo.number ?? "")} ${escapeHtml(prInfo.title ?? "Pull request")}</strong>
      <span>${escapeHtml([prInfo.state, prInfo.isDraft ? "draft" : "", `${prInfo.headRefName ?? "head"} → ${prInfo.baseRefName ?? "base"}`].filter(Boolean).join(" · "))}</span>
      ${prInfo.url ? `<a href="${escapeAttr(prInfo.url)}">${escapeHtml(prInfo.url)}</a>` : ""}
      <button data-action-click="open-pr-modal">Edit PR</button>
    </div>
  `;
}

function renderChecksPanel() {
  const checks = prInfo?.checks ?? [];
  const actions = `
    <div class="checks-actions">
      <button data-action-click="refresh-pr">Refresh</button>
      <button data-action-click="rerun-failed-checks" ${checks.length ? "" : "disabled"}>Rerun failed</button>
    </div>
  `;
  if (prInfo?.error) {
    return `${actions}<div class="panel-empty">No checks yet<br><span>${escapeHtml(prInfo.error)}</span></div>`;
  }
  if (!checks.length) {
    return `${actions}<div class="panel-empty">No checks yet<br><span>Checks from GitHub will appear here.</span></div>`;
  }
  return actions + checks.map(renderCheckRow).join("");
}

function renderCheckRow(check) {
  const status = checkStateLabel(check);
  const rows = [
    ["Workflow", check.workflowName],
    ["Kind", check.kind],
    ["Status", check.status],
    ["Conclusion", check.conclusion],
    ["Started", formatCheckTime(check.startedAt)],
    ["Completed", formatCheckTime(check.completedAt)],
    ["Duration", formatCheckDuration(check.startedAt, check.completedAt)]
  ].filter(([, value]) => value);
  return `
    <details class="check-row ${escapeAttr(checkTone(check))}">
      <summary>
        <span>${escapeHtml(status)}</span>
        <strong>${escapeHtml(check.name || "check")}</strong>
        ${check.detailsUrl ? `<a href="${escapeAttr(check.detailsUrl)}">details</a>` : ""}
      </summary>
      <div class="check-detail-grid">
        ${
          rows
            .map(([label, value]) => `<small>${escapeHtml(label)}</small><code>${escapeHtml(value)}</code>`)
            .join("") || `<small>Evidence</small><code>No extra check metadata returned by GitHub.</code>`
        }
      </div>
    </details>
  `;
}

function checkStateLabel(check) {
  return check.conclusion || check.status || "PENDING";
}

function checkTone(check) {
  const value = checkStateLabel(check).toLowerCase();
  if (value === "success" || value === "passed") return "success";
  if (["failure", "failed", "error", "cancelled", "timed_out"].includes(value)) return "failure";
  if (["pending", "queued", "in_progress", "requested", "waiting", "expected"].includes(value)) return "pending";
  return value || "pending";
}

function formatCheckTime(value) {
  if (!value) return "";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function formatCheckDuration(startedAt, completedAt) {
  if (!startedAt || !completedAt) return "";
  const started = new Date(startedAt).getTime();
  const completed = new Date(completedAt).getTime();
  if (Number.isNaN(started) || Number.isNaN(completed) || completed < started) return "";
  const seconds = Math.round((completed - started) / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const remainder = seconds % 60;
  return remainder ? `${minutes}m ${remainder}s` : `${minutes}m`;
}

function renderChangesPanel() {
  if (!changes.length && !diffFiles.length) {
    return `<div class="panel-empty">No file changes yet<br><span>Review code changes here.</span></div>`;
  }
  const changeItems = changes.length ? changes : diffFiles;
  const query = changeFilter.trim().toLowerCase();
  const visibleItems = query
    ? changeItems.filter((item) => item.path.toLowerCase().includes(query))
    : changeItems;
  const selected =
    diffFiles.find((file) => file.path === selectedDiffPath) ??
    diffFiles.find((file) => visibleItems.some((item) => item.path === file.path)) ??
    diffFiles[0];
  const totalAdditions = diffFiles.reduce((total, file) => total + (file.additions || 0), 0);
  const totalDeletions = diffFiles.reduce((total, file) => total + (file.deletions || 0), 0);
  return `
    <div class="change-layout">
      <div class="change-summary">
        <strong>${escapeHtml(diffFiles.length || changes.length)} files changed</strong>
        <span>+${escapeHtml(totalAdditions)} -${escapeHtml(totalDeletions)}</span>
      </div>
      <div class="change-filter">
        <input data-action="change-filter" value="${escapeAttr(changeFilter)}" placeholder="Filter changed files" />
        <span>${escapeHtml(visibleItems.length)} / ${escapeHtml(changeItems.length)}</span>
      </div>
      <div class="change-files">
        ${visibleItems.length ? visibleItems
          .map((item) => {
            const path = item.path;
            return `
              <button class="change-file ${path === selected?.path ? "active" : ""}" data-diff-file="${escapeAttr(path)}">
                <span>${escapeHtml(item.kind ?? item.status ?? "changed")}</span>
                ${escapeHtml(path)}
              </button>
            `;
          })
          .join("") : `<div class="panel-empty small">No matching changes</div>`}
      </div>
      ${
        selected
          ? renderPatchView(selected)
          : `<div class="panel-empty">No patch available</div>`
      }
    </div>
  `;
}

function renderPatchView(selected) {
  const hunks = patchHunks(selected.patch);
  const activeIndex = Math.min(selectedHunkIndex, hunks.length - 1);
  const activeHunk = hunks[activeIndex] ?? { title: "Full patch", patch: selected.patch };
  return `
    <div class="patch-view">
      <div class="patch-title">
        <strong>${escapeHtml(selected.path)}</strong>
        <span>+${selected.additions} -${selected.deletions}</span>
        <div>
          <button type="button" data-action-click="open-selected-diff-file">Open</button>
          <button type="button" data-action-click="copy-selected-patch">Copy patch</button>
          <button type="button" data-action-click="reveal-selected-diff-file">Reveal</button>
        </div>
      </div>
      ${
        hunks.length > 1
          ? `
            <div class="hunk-nav">
              ${hunks
                .map(
                  (hunk, index) => `
                    <button class="${index === activeIndex ? "active" : ""}" data-select-hunk="${index}">
                      ${escapeHtml(hunk.title || `Hunk ${index + 1}`)}
                    </button>
                  `
                )
                .join("")}
            </div>
          `
          : ""
      }
      <div class="patch-code">${renderPatch(activeHunk.patch)}</div>
      <form class="comment-form" data-action="add-comment" data-file="${escapeAttr(selected.path)}" data-line-number="${escapeAttr(selectedCommentLine || 0)}">
        <input name="body" placeholder="${escapeAttr(selectedCommentLine ? `Leave a comment on line ${selectedCommentLine}` : "Select a line or leave a file comment")}" />
        <button type="submit">Comment</button>
      </form>
      <div class="comments">
        ${diffComments
          .filter((comment) => comment.filePath === selected.path)
          .map(
            (comment) => `
              <article class="diff-comment ${comment.isResolved ? "resolved" : ""}">
                <p>${comment.lineNumber ? `<code>L${escapeHtml(comment.lineNumber)}</code> ` : ""}${escapeHtml(comment.body)}</p>
                <button data-resolve-comment="${escapeAttr(comment.id)}" ${comment.isResolved ? "disabled" : ""}>${comment.isResolved ? "Resolved" : "Resolve"}</button>
              </article>
            `
          )
          .join("")}
      </div>
    </div>
  `;
}

function patchHunks(patch) {
  const lines = patch.split("\n");
  const header = [];
  const hunks = [];
  let current = null;
  for (const line of lines) {
    if (line.startsWith("@@")) {
      if (current) hunks.push(current);
      current = { title: line.replace(/^@@\s*/, "").replace(/\s*@@.*$/, ""), lines: [...header, line] };
    } else if (current) {
      current.lines.push(line);
    } else {
      header.push(line);
    }
  }
  if (current) hunks.push(current);
  if (!hunks.length) return [{ title: "Full patch", patch }];
  return hunks.map((hunk, index) => ({
    title: hunk.title || `Hunk ${index + 1}`,
    patch: hunk.lines.join("\n")
  }));
}

function renderPatch(patch) {
  let oldLine = 0;
  let newLine = 0;
  return patch
    .split("\n")
    .map((line) => {
      const hunk = line.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      if (hunk) {
        oldLine = Number(hunk[1]);
        newLine = Number(hunk[2]);
        return `<div class="patch-line line-hunk"><span class="line-no"></span><span class="line-text">${escapeHtml(line || " ")}</span></div>`;
      }
      let lineNumber = 0;
      let className = "";
      if (line.startsWith("+") && !line.startsWith("+++")) {
        lineNumber = newLine++;
        className = "line-add";
      } else if (line.startsWith("-") && !line.startsWith("---")) {
        lineNumber = oldLine++;
        className = "line-del";
      } else {
        lineNumber = newLine || 0;
        if (newLine) newLine += 1;
        if (oldLine) oldLine += 1;
      }
      const active = lineNumber && lineNumber === selectedCommentLine ? "active" : "";
      const clickAttr = lineNumber ? `data-comment-line="${escapeAttr(lineNumber)}"` : "";
      return `
        <button type="button" class="patch-line ${className} ${active}" ${clickAttr}>
          <span class="line-no">${lineNumber || ""}</span>
          <span class="line-text">${escapeHtml(line || " ")}</span>
        </button>
      `;
    })
    .join("\n");
}

function defaultModelFor(agentType) {
  return agentType === "codex"
    ? (settings?.defaultCodexModel || "gpt-5-codex")
    : (settings?.defaultClaudeModel || "opus");
}

function agentLabel(agentType) {
  return agentType === "codex" ? "Codex" : "Claude";
}

function modelLabel(agentType) {
  return agentType === "codex" ? "Model" : "Opus";
}

function effortLabel(agentType) {
  if (agentType === "codex") return titleCase(settings?.defaultCodexEffort || "high");
  return "High";
}

function modelOptions(session) {
  const current = session?.model || defaultModelFor(session?.agentType);
  const options = session?.agentType === "codex"
    ? [
        ["gpt-5-codex", "GPT-5 Codex"],
        ["gpt-5.2", "GPT-5.2"],
        ["gpt-5", "GPT-5"]
      ]
    : [
        ["opus", "Opus 4.7"],
        ["sonnet", "Sonnet 4.5"],
        ["haiku", "Haiku"]
      ];
  if (!options.some(([value]) => value === current)) {
    options.unshift([current, current]);
  }
  return options
    .map(([value, label]) => `<option value="${escapeAttr(value)}" ${value === current ? "selected" : ""}>${escapeHtml(label)}</option>`)
    .join("");
}

function permissionOptions(current) {
  const labels = {
    default: "Default",
    acceptEdits: "Accept edits",
    auto: "Auto",
    dontAsk: "Don't ask",
    plan: "Plan",
    bypassPermissions: "Bypass"
  };
  return Object.keys(labels)
    .map((value) => `<option value="${escapeAttr(value)}" ${value === current ? "selected" : ""}>${escapeHtml(labels[value])}</option>`)
    .join("");
}

function shortPath(path) {
  if (!path) return "";
  const home = path.replace(/^\/Users\/[^/]+/, "~");
  if (home.startsWith("~/")) {
    const pieces = home.slice(2).split("/").filter(Boolean);
    return pieces.length <= 1 ? home : `~/${pieces.slice(-2).join("/")}`;
  }
  const pieces = home.split("/").filter(Boolean);
  if (pieces.length <= 2) return home;
  return `/${pieces.slice(-2).join("/")}`;
}

function titleCase(value) {
  value = String(value || "");
  return value ? value[0].toUpperCase() + value.slice(1) : value;
}

function formatNumber(value) {
  return new Intl.NumberFormat("en-US", { maximumFractionDigits: 0 }).format(value ?? 0);
}

function relativeTime(time) {
  if (!time) return "";
  const seconds = Math.max(0, Math.round((Date.now() - time) / 1000));
  if (seconds < 5) return "just now";
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  return `${Math.round(minutes / 60)}h ago`;
}

function escapeHtml(value) {
  return String(value).replace(/[&<>"']/g, (char) => {
    const entities = {
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#039;"
    };
    return entities[char];
  });
}

function escapeAttr(value) {
  return escapeHtml(value).replace(/`/g, "&#096;");
}

load().catch((error) => {
  app.innerHTML = `<pre class="boot-error">${escapeHtml(String(error))}</pre>`;
});
