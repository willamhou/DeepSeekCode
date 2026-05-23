const path = require("path");
const childProcess = require("child_process");
const fs = require("fs");
const os = require("os");
const util = require("util");
const vscode = require("vscode");

const execFile = util.promisify(childProcess.execFile);

function activate(context) {
  const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  status.text = "$(sparkle) DeepseekCode";
  status.tooltip = "DeepseekCode actions";
  status.command = "deepseek.quickAction";
  status.show();

  const panelProvider = new DeepseekPanelProvider();

  context.subscriptions.push(
    status,
    vscode.window.registerTreeDataProvider("deepseek.actions", new DeepseekActionsProvider()),
    vscode.window.registerWebviewViewProvider("deepseek.panel", panelProvider),
    vscode.commands.registerCommand("deepseek.quickAction", quickAction),
    vscode.commands.registerCommand("deepseek.openPanel", openPanel),
    vscode.commands.registerCommand("deepseek.openChat", openChat),
    vscode.commands.registerCommand("deepseek.runTask", runTask),
    vscode.commands.registerCommand("deepseek.explainSelection", explainSelection),
    vscode.commands.registerCommand("deepseek.explainDiagnostics", explainDiagnostics),
    vscode.commands.registerCommand("deepseek.showActiveDiff", showActiveDiff),
    vscode.commands.registerCommand("deepseek.runBenchmark", runBenchmark),
    vscode.commands.registerCommand("deepseek.showDogfoodReport", showDogfoodReport),
  );

  return {
    createTestPanelProvider: () => new DeepseekPanelProvider(),
    panelProvider,
  };
}

function deactivate() {}

class DeepseekPanelProvider {
  constructor() {
    this.currentRun = undefined;
    this.reviewedPaths = new Set();
    this.generatedPatches = [];
    this.nextGeneratedPatchId = 1;
  }

  resolveWebviewView(view) {
    view.webview.options = {
      enableScripts: true,
    };
    view.webview.html = panelHtml(nonce());
    view.webview.onDidReceiveMessage(async (message) => {
      switch (message?.type) {
        case "openChat":
          await openChat();
          break;
        case "runTask":
          await this.runPanelTask(view, message.task);
          break;
        case "resumeLatest":
          await this.resumeLatest(view, message.task);
          break;
        case "cancelTask":
          this.cancelPanelTask(view);
          break;
        case "explainSelection":
          await explainSelection();
          break;
        case "explainDiagnostics":
          await explainDiagnostics();
          break;
        case "showActiveDiff":
          await showActiveDiff();
          break;
        case "reviewDiff":
          await showActiveDiff();
          await this.refreshPanelDiff(view);
          break;
        case "refreshDiff":
          await this.refreshPanelDiff(view);
          break;
        case "refreshPatchQueue":
          await this.refreshPatchQueue(view);
          break;
        case "refreshGeneratedPatches":
          this.refreshGeneratedPatches(view);
          break;
        case "openPatchFile":
          await this.openPatchFile(view, message.path);
          break;
        case "acceptPatchFile":
          await this.acceptPatchFile(view, message.path);
          break;
        case "revertPatchFile":
          await this.revertPatchFile(view, message.path);
          break;
        case "openGeneratedPatch":
          await this.openGeneratedPatch(view, message.id);
          break;
        case "applyGeneratedPatch":
          await this.applyGeneratedPatch(view, message.id);
          break;
        case "rejectGeneratedPatch":
          this.rejectGeneratedPatch(view, message.id);
          break;
        case "acceptDiff":
          await this.acceptPanelDiff(view);
          break;
        case "revertActiveFile":
          await this.revertActiveFile(view);
          break;
        case "validate":
          await this.runValidation(view, message.command);
          break;
        case "runBenchmark":
          await runBenchmark();
          break;
        case "showDogfoodReport":
          await showDogfoodReport();
          break;
      }
    });
  }

  async runPanelTask(view, task) {
    if (!task || !task.trim()) {
      vscode.window.showInformationMessage("Enter a DeepseekCode task before running.");
      return;
    }
    if (this.currentRun) {
      vscode.window.showInformationMessage("A DeepseekCode panel task is already running.");
      return;
    }

    const cwd = workspaceCwd();
    const prompt = await promptWithWorkbenchContext(task.trim());
    this.startPanelExec(view, ["exec", "--json", "--", prompt], cwd, task.trim());
  }

  async resumeLatest(view, task) {
    if (this.currentRun) {
      vscode.window.showInformationMessage("A DeepseekCode panel task is already running.");
      return;
    }
    const cwd = workspaceCwd();
    const args = ["exec", "resume", "--json"];
    if (task && task.trim()) {
      args.push("--", await promptWithWorkbenchContext(task.trim()));
    }
    this.startPanelExec(view, args, cwd, task && task.trim() ? task.trim() : "Resume latest session");
  }

  startPanelExec(view, args, cwd, taskLabel) {
    const child = spawnDeepseek(args, cwd);
    const run = {
      child,
      stdout: "",
      stderr: "",
      startedAt: Date.now(),
    };
    this.currentRun = run;
    view.webview.postMessage({
      type: "runStarted",
      cwd: cwd || "",
      command: deepseekCommand(),
      task: taskLabel,
    });
    this.refreshGeneratedPatches(view);

    child.stdout.on("data", (chunk) => {
      run.stdout += chunk.toString();
      run.stdout = drainJsonLines(run.stdout, (line) => {
        postExecJsonLine(view, line, (event) => this.collectGeneratedPatchesFromEvent(view, event));
      });
    });
    child.stderr.on("data", (chunk) => {
      const text = chunk.toString();
      run.stderr += text;
      view.webview.postMessage({ type: "log", level: "stderr", text });
    });
    child.on("error", (error) => {
      if (this.currentRun === run) {
        this.currentRun = undefined;
      }
      view.webview.postMessage({ type: "runEnded", ok: false, error: error.message });
    });
    child.on("close", async (code, signal) => {
      if (run.stdout.trim()) {
        drainJsonLines(`${run.stdout}\n`, (line) => {
          postExecJsonLine(view, line, (event) => this.collectGeneratedPatchesFromEvent(view, event));
        });
      }
      if (this.currentRun === run) {
        this.currentRun = undefined;
      }
      view.webview.postMessage({
        type: "runEnded",
        ok: code === 0,
        code,
        signal,
        elapsedMs: Date.now() - run.startedAt,
      });
      await this.refreshPanelDiff(view);
      await this.refreshPatchQueue(view);
    });
  }

  cancelPanelTask(view) {
    if (!this.currentRun) {
      return;
    }
    const { child } = this.currentRun;
    this.currentRun = undefined;
    child.kill();
    view.webview.postMessage({ type: "log", level: "status", text: "Cancellation requested.\n" });
  }

  async refreshPanelDiff(view) {
    const review = await activeFilePatchReview();
    view.webview.postMessage({ type: "patchReview", ...review });
  }

  async refreshPatchQueue(view) {
    const queue = await workspacePatchQueue(this.reviewedPaths);
    view.webview.postMessage({ type: "patchQueue", ...queue });
  }

  refreshGeneratedPatches(view) {
    view.webview.postMessage({
      type: "generatedPatchQueue",
      patches: this.generatedPatches,
      summary: generatedPatchQueueSummary(this.generatedPatches),
    });
  }

  async acceptPanelDiff(view) {
    const review = await activeFilePatchReview();
    if (!review.hasDiff) {
      view.webview.postMessage({ type: "log", level: "review", text: "No active-file diff to accept.\n" });
      return;
    }
    if (review.gitRoot && review.path) {
      this.reviewedPaths.add(reviewedPathKey(review.gitRoot, review.path));
    }
    view.webview.postMessage({
      type: "patchAccepted",
      summary: review.summary,
    });
    await this.refreshPatchQueue(view);
  }

  async revertActiveFile(view) {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.uri.scheme !== "file") {
      vscode.window.showInformationMessage("Open a file-backed editor before reverting changes.");
      return;
    }
    const review = await activeFilePatchReview();
    if (!review.hasDiff) {
      view.webview.postMessage({ type: "log", level: "review", text: "No active-file diff to revert.\n" });
      return;
    }
    if (review.untracked) {
      vscode.window.showInformationMessage("The active file is untracked; remove it manually if you want to discard it.");
      return;
    }
    const picked = await vscode.window.showWarningMessage(
      `Revert changes to ${review.path}?`,
      { modal: true },
      "Revert",
    );
    if (picked !== "Revert") {
      return;
    }
    try {
      await execFile("git", ["checkout", "HEAD", "--", review.path], {
        cwd: review.gitRoot,
        maxBuffer: 10 * 1024 * 1024,
      });
      view.webview.postMessage({ type: "patchReview", hasDiff: false, summary: "Reverted active file." });
      await this.refreshPatchQueue(view);
    } catch (error) {
      view.webview.postMessage({ type: "log", level: "error", text: `${error.message}\n` });
    }
  }

  async openPatchFile(view, filePath) {
    const queue = await workspacePatchQueue(this.reviewedPaths);
    const item = queue.files.find((file) => file.path === filePath);
    if (!item) {
      view.webview.postMessage({ type: "log", level: "review", text: `No queued file: ${filePath}\n` });
      return;
    }
    await showWorkspaceFileDiff(queue.gitRoot, item);
  }

  async acceptPatchFile(view, filePath) {
    const queue = await workspacePatchQueue(this.reviewedPaths);
    const item = queue.files.find((file) => file.path === filePath);
    if (!item) {
      view.webview.postMessage({ type: "log", level: "review", text: `No queued file: ${filePath}\n` });
      return;
    }
    this.reviewedPaths.add(reviewedPathKey(queue.gitRoot, filePath));
    await this.refreshPatchQueue(view);
  }

  async revertPatchFile(view, filePath) {
    const queue = await workspacePatchQueue(this.reviewedPaths);
    const item = queue.files.find((file) => file.path === filePath);
    if (!item) {
      view.webview.postMessage({ type: "log", level: "review", text: `No queued file: ${filePath}\n` });
      return;
    }
    if (item.untracked) {
      vscode.window.showInformationMessage("The selected file is untracked; remove it manually if you want to discard it.");
      return;
    }
    const picked = await vscode.window.showWarningMessage(
      `Revert changes to ${filePath}?`,
      { modal: true },
      "Revert",
    );
    if (picked !== "Revert") {
      return;
    }
    try {
      await execFile("git", ["checkout", "HEAD", "--", filePath], {
        cwd: queue.gitRoot,
        maxBuffer: 10 * 1024 * 1024,
      });
      this.reviewedPaths.delete(reviewedPathKey(queue.gitRoot, filePath));
      await this.refreshPatchQueue(view);
      await this.refreshPanelDiff(view);
    } catch (error) {
      view.webview.postMessage({ type: "log", level: "error", text: `${error.message}\n` });
    }
  }

  async openGeneratedPatch(view, patchId) {
    const patch = this.generatedPatches.find((item) => String(item.id) === String(patchId));
    if (!patch) {
      view.webview.postMessage({ type: "log", level: "patch", text: `No generated patch: ${patchId}\n` });
      return;
    }
    try {
      const gitRoot = await gitRootForWorkspace();
      const opened = await showGeneratedPatchDiff(gitRoot, patch);
      if (opened) {
        return;
      }
    } catch (error) {
      view.webview.postMessage({
        type: "log",
        level: "patch",
        text: `Generated patch diff preview unavailable: ${error.message}\n`,
      });
    }
    const document = await vscode.workspace.openTextDocument({
      content: patch.patch,
      language: "diff",
    });
    await vscode.window.showTextDocument(document);
  }

  async applyGeneratedPatch(view, patchId) {
    const patch = this.generatedPatches.find((item) => String(item.id) === String(patchId));
    if (!patch) {
      view.webview.postMessage({ type: "log", level: "patch", text: `No generated patch: ${patchId}\n` });
      return;
    }
    if (patch.status !== "pending") {
      view.webview.postMessage({
        type: "log",
        level: "patch",
        text: `Generated patch ${patch.id} is already ${patch.status}.\n`,
      });
      return;
    }
    let gitRoot;
    try {
      gitRoot = await gitRootForWorkspace();
    } catch (error) {
      view.webview.postMessage({ type: "log", level: "error", text: `${error.message}\n` });
      return;
    }
    const picked = await vscode.window.showWarningMessage(
      `Apply generated patch ${patch.id}?`,
      { modal: true },
      "Apply",
    );
    if (picked !== "Apply") {
      return;
    }
    try {
      await runGitApplyChecked(gitRoot, patch.patch);
      patch.status = "applied";
      patch.updatedAt = Date.now();
      view.webview.postMessage({
        type: "log",
        level: "patch",
        text: `Applied generated patch ${patch.id}: ${patch.summary}\n`,
      });
      this.refreshGeneratedPatches(view);
      await this.refreshPatchQueue(view);
      await this.refreshPanelDiff(view);
    } catch (error) {
      view.webview.postMessage({
        type: "log",
        level: "error",
        text: `Generated patch ${patch.id} failed: ${error.message}\n`,
      });
    }
  }

  rejectGeneratedPatch(view, patchId) {
    const patch = this.generatedPatches.find((item) => String(item.id) === String(patchId));
    if (!patch) {
      view.webview.postMessage({ type: "log", level: "patch", text: `No generated patch: ${patchId}\n` });
      return;
    }
    patch.status = "rejected";
    patch.updatedAt = Date.now();
    this.refreshGeneratedPatches(view);
  }

  collectGeneratedPatchesFromEvent(view, event) {
    const artifacts = generatedPatchesFromExecEvent(event);
    if (artifacts.length === 0) {
      return;
    }
    let added = 0;
    for (const artifact of artifacts) {
      const patch = normalizeGeneratedPatch(artifact.patch);
      if (!patch || this.generatedPatches.some((item) => item.patch === patch)) {
        continue;
      }
      this.generatedPatches.unshift({
        id: this.nextGeneratedPatchId,
        source: artifact.source,
        patch,
        summary: generatedPatchSummary(patch),
        files: generatedPatchFiles(patch),
        status: artifact.status || "pending",
        createdAt: Date.now(),
      });
      this.nextGeneratedPatchId += 1;
      added += 1;
    }
    if (added > 0) {
      this.refreshGeneratedPatches(view);
    }
  }

  async runValidation(view, command) {
    if (!command || !command.trim()) {
      vscode.window.showInformationMessage("Enter a validation command first.");
      return;
    }
    if (this.currentRun) {
      vscode.window.showInformationMessage("A DeepseekCode panel task is already running.");
      return;
    }
    const cwd = workspaceCwd();
    const child = childProcess.spawn(command.trim(), [], {
      cwd,
      shell: true,
      env: process.env,
    });
    const run = {
      child,
      stdout: "",
      stderr: "",
      startedAt: Date.now(),
    };
    this.currentRun = run;
    view.webview.postMessage({ type: "validationStarted", command: command.trim(), cwd: cwd || "" });
    child.stdout.on("data", (chunk) => {
      const text = chunk.toString();
      run.stdout += text;
      view.webview.postMessage({ type: "log", level: "validate", text });
    });
    child.stderr.on("data", (chunk) => {
      const text = chunk.toString();
      run.stderr += text;
      view.webview.postMessage({ type: "log", level: "validate", text });
    });
    child.on("error", (error) => {
      if (this.currentRun === run) {
        this.currentRun = undefined;
      }
      view.webview.postMessage({ type: "validationEnded", ok: false, error: error.message });
    });
    child.on("close", (code, signal) => {
      if (this.currentRun === run) {
        this.currentRun = undefined;
      }
      view.webview.postMessage({
        type: "validationEnded",
        ok: code === 0,
        code,
        signal,
        elapsedMs: Date.now() - run.startedAt,
      });
    });
  }
}

class DeepseekActionsProvider {
  getTreeItem(item) {
    return item;
  }

  getChildren() {
    return [
      actionItem(
        "Open Chat",
        "Start an interactive session",
        "deepseek.openChat",
        "comment-discussion",
      ),
      actionItem("Run Task", "Prompt for a workspace task", "deepseek.runTask", "terminal"),
      actionItem(
        "Explain Selection",
        "Send active file and selection as context",
        "deepseek.explainSelection",
        "symbol-method",
      ),
      actionItem(
        "Explain Diagnostics",
        "Send active file diagnostics as task context",
        "deepseek.explainDiagnostics",
        "warning",
      ),
      actionItem(
        "Show Active Diff",
        "Open a review diff for the active file",
        "deepseek.showActiveDiff",
        "diff",
      ),
      actionItem(
        "Run Benchmark",
        "Run the local benchmark suite",
        "deepseek.runBenchmark",
        "beaker",
      ),
      actionItem(
        "Show Dogfood Report",
        "Show recent dogfood runs",
        "deepseek.showDogfoodReport",
        "graph",
      ),
    ];
  }
}

function actionItem(label, description, command, codicon) {
  const item = new vscode.TreeItem(label, vscode.TreeItemCollapsibleState.None);
  item.description = description;
  item.tooltip = description;
  item.iconPath = new vscode.ThemeIcon(codicon);
  item.command = {
    command,
    title: label,
  };
  return item;
}

function panelHtml(panelNonce) {
  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'nonce-${panelNonce}';">
  <style>
    body {
      box-sizing: border-box;
      color: var(--vscode-foreground);
      font-family: var(--vscode-font-family);
      margin: 0;
      padding: 12px;
    }
    .stack {
      display: flex;
      flex-direction: column;
      gap: 8px;
    }
    textarea {
      background: var(--vscode-input-background);
      border: 1px solid var(--vscode-input-border);
      box-sizing: border-box;
      color: var(--vscode-input-foreground);
      font-family: var(--vscode-font-family);
      min-height: 96px;
      padding: 8px;
      resize: vertical;
      width: 100%;
    }
    button {
      align-items: center;
      background: var(--vscode-button-secondaryBackground);
      border: 0;
      color: var(--vscode-button-secondaryForeground);
      cursor: pointer;
      display: flex;
      font: inherit;
      justify-content: center;
      min-height: 28px;
      padding: 5px 8px;
      text-align: center;
      width: 100%;
    }
    button.primary {
      background: var(--vscode-button-background);
      color: var(--vscode-button-foreground);
    }
    button:hover {
      background: var(--vscode-button-hoverBackground);
    }
    .grid {
      display: grid;
      gap: 8px;
      grid-template-columns: 1fr 1fr;
    }
    .status {
      color: var(--vscode-descriptionForeground);
      min-height: 18px;
      overflow-wrap: anywhere;
    }
    .pane {
      border: 1px solid var(--vscode-panel-border);
      min-height: 64px;
      overflow: auto;
      padding: 8px;
    }
    .assistant {
      white-space: pre-wrap;
    }
    .tool {
      border-bottom: 1px solid var(--vscode-panel-border);
      padding: 6px 0;
    }
    .tool:last-child {
      border-bottom: 0;
    }
    .tool-name {
      color: var(--vscode-symbolIcon-functionForeground);
      font-weight: 600;
    }
    .tool-meta {
      color: var(--vscode-descriptionForeground);
      overflow-wrap: anywhere;
    }
    .logs {
      color: var(--vscode-descriptionForeground);
      max-height: 140px;
      white-space: pre-wrap;
    }
    .review {
      color: var(--vscode-descriptionForeground);
      white-space: pre-wrap;
    }
    .queue-list {
      display: flex;
      flex-direction: column;
      gap: 6px;
    }
    .queue-item {
      border-bottom: 1px solid var(--vscode-panel-border);
      display: grid;
      gap: 4px;
      grid-template-columns: 1fr;
      padding: 6px 0;
    }
    .queue-item:last-child {
      border-bottom: 0;
    }
    .queue-path {
      overflow-wrap: anywhere;
    }
    .queue-actions {
      display: grid;
      gap: 6px;
      grid-template-columns: repeat(3, 1fr);
    }
    .validation {
      display: grid;
      gap: 8px;
      grid-template-columns: 1fr auto;
    }
    input {
      background: var(--vscode-input-background);
      border: 1px solid var(--vscode-input-border);
      box-sizing: border-box;
      color: var(--vscode-input-foreground);
      font-family: var(--vscode-font-family);
      min-width: 0;
      padding: 6px 8px;
    }
    .validation button {
      min-width: 84px;
      width: auto;
    }
  </style>
</head>
<body>
  <div class="stack">
    <textarea id="task" aria-label="Task" placeholder="Task"></textarea>
    <button class="primary" id="run">Run</button>
    <button id="resume">Resume Latest</button>
    <button id="cancel">Cancel</button>
    <div class="status" id="status"></div>
    <div class="pane assistant" id="assistant"></div>
    <div class="pane" id="tools"></div>
    <div class="pane review" id="review"></div>
    <div class="grid">
      <button id="reviewDiff">Review Diff</button>
      <button id="acceptDiff">Accept</button>
      <button id="revertFile">Revert File</button>
      <button id="refreshDiff">Refresh Diff</button>
    </div>
    <div class="pane review">
      <div id="patchQueue"></div>
    </div>
    <button id="refreshPatchQueue">Workspace Changes</button>
    <div class="pane review">
      <div id="generatedPatchQueue"></div>
    </div>
    <button id="refreshGeneratedPatches">Generated Patches</button>
    <div class="validation">
      <input id="validationCommand" aria-label="Validation command" placeholder="Validation command">
      <button id="validate">Validate</button>
    </div>
    <div class="pane logs" id="logs"></div>
    <div class="grid">
      <button id="chat">Chat</button>
      <button id="explain">Explain</button>
      <button id="diagnostics">Diagnostics</button>
      <button id="diff">Diff</button>
      <button id="benchmark">Benchmark</button>
      <button id="dogfood">Dogfood</button>
    </div>
  </div>
  <script nonce="${panelNonce}">
    const vscode = acquireVsCodeApi();
    const task = document.getElementById("task");
    document.getElementById("run").addEventListener("click", () => {
      vscode.postMessage({ type: "runTask", task: task.value });
    });
    document.getElementById("resume").addEventListener("click", () => {
      vscode.postMessage({ type: "resumeLatest", task: task.value });
    });
    document.getElementById("cancel").addEventListener("click", () => {
      vscode.postMessage({ type: "cancelTask" });
    });
    document.getElementById("chat").addEventListener("click", () => {
      vscode.postMessage({ type: "openChat" });
    });
    document.getElementById("explain").addEventListener("click", () => {
      vscode.postMessage({ type: "explainSelection" });
    });
    document.getElementById("diagnostics").addEventListener("click", () => {
      vscode.postMessage({ type: "explainDiagnostics" });
    });
    document.getElementById("diff").addEventListener("click", () => {
      vscode.postMessage({ type: "showActiveDiff" });
    });
    document.getElementById("reviewDiff").addEventListener("click", () => {
      vscode.postMessage({ type: "reviewDiff" });
    });
    document.getElementById("acceptDiff").addEventListener("click", () => {
      vscode.postMessage({ type: "acceptDiff" });
    });
    document.getElementById("revertFile").addEventListener("click", () => {
      vscode.postMessage({ type: "revertActiveFile" });
    });
    document.getElementById("refreshDiff").addEventListener("click", () => {
      vscode.postMessage({ type: "refreshDiff" });
    });
    document.getElementById("refreshPatchQueue").addEventListener("click", () => {
      vscode.postMessage({ type: "refreshPatchQueue" });
    });
    document.getElementById("refreshGeneratedPatches").addEventListener("click", () => {
      vscode.postMessage({ type: "refreshGeneratedPatches" });
    });
    document.getElementById("validate").addEventListener("click", () => {
      vscode.postMessage({
        type: "validate",
        command: document.getElementById("validationCommand").value,
      });
    });
    document.getElementById("benchmark").addEventListener("click", () => {
      vscode.postMessage({ type: "runBenchmark" });
    });
    document.getElementById("dogfood").addEventListener("click", () => {
      vscode.postMessage({ type: "showDogfoodReport" });
    });
    const status = document.getElementById("status");
    const assistant = document.getElementById("assistant");
    const tools = document.getElementById("tools");
    const review = document.getElementById("review");
    const patchQueue = document.getElementById("patchQueue");
    const generatedPatchQueue = document.getElementById("generatedPatchQueue");
    const logs = document.getElementById("logs");
    window.addEventListener("message", (event) => {
      const message = event.data || {};
      if (message.type === "runStarted") {
        status.textContent = "Running in " + (message.cwd || "workspace");
        assistant.textContent = "";
        tools.textContent = "";
        review.textContent = "";
        patchQueue.textContent = "";
        logs.textContent = "";
      } else if (message.type === "assistantDelta") {
        assistant.textContent += message.text || "";
        assistant.scrollTop = assistant.scrollHeight;
      } else if (message.type === "reasoningDelta") {
        appendLog("reasoning", message.text || "");
      } else if (message.type === "toolEvent") {
        appendTool(message);
      } else if (message.type === "final") {
        if (message.message) {
          assistant.textContent = message.message;
        }
        status.textContent = "Done";
      } else if (message.type === "log") {
        appendLog(message.level || "log", message.text || "");
      } else if (message.type === "runEnded") {
        status.textContent = message.ok ? "Completed" : "Failed";
        if (message.error) {
          appendLog("error", message.error + "\\n");
        }
      } else if (message.type === "patchReview") {
        review.textContent = message.hasDiff
          ? (message.summary || "Active file has changes.")
          : (message.summary || "No active-file diff.");
      } else if (message.type === "patchAccepted") {
        review.textContent = "Accepted active-file diff.\\n" + (message.summary || "");
      } else if (message.type === "patchQueue") {
        renderPatchQueue(message);
      } else if (message.type === "generatedPatchQueue") {
        renderGeneratedPatchQueue(message);
      } else if (message.type === "validationStarted") {
        status.textContent = "Validating";
        appendLog("validate", "$ " + (message.command || "") + "\\n");
      } else if (message.type === "validationEnded") {
        status.textContent = message.ok ? "Validation passed" : "Validation failed";
        if (message.error) {
          appendLog("error", message.error + "\\n");
        }
      }
    });
    function appendTool(message) {
      const item = document.createElement("div");
      item.className = "tool";
      const name = document.createElement("div");
      name.className = "tool-name";
      name.textContent = message.tool || "tool";
      const meta = document.createElement("div");
      meta.className = "tool-meta";
      meta.textContent = message.detail || "";
      item.appendChild(name);
      item.appendChild(meta);
      tools.appendChild(item);
      tools.scrollTop = tools.scrollHeight;
    }
    function appendLog(level, text) {
      logs.textContent += "[" + level + "] " + text;
      logs.scrollTop = logs.scrollHeight;
    }
    function renderPatchQueue(message) {
      patchQueue.textContent = "";
      if (!message.files || message.files.length === 0) {
        patchQueue.textContent = message.summary || "No workspace changes.";
        return;
      }
      const heading = document.createElement("div");
      heading.textContent = message.summary || "Workspace changes";
      patchQueue.appendChild(heading);
      const list = document.createElement("div");
      list.className = "queue-list";
      message.files.forEach((file) => {
        const item = document.createElement("div");
        item.className = "queue-item";
        const label = document.createElement("div");
        label.className = "queue-path";
        label.textContent = (file.reviewed ? "[reviewed] " : "") + file.status + " " + file.path;
        const actions = document.createElement("div");
        actions.className = "queue-actions";
        addQueueButton(actions, "Open", "openPatchFile", file.path);
        addQueueButton(actions, "Accept", "acceptPatchFile", file.path);
        addQueueButton(actions, "Revert", "revertPatchFile", file.path);
        item.appendChild(label);
        item.appendChild(actions);
        list.appendChild(item);
      });
      patchQueue.appendChild(list);
    }
    function addQueueButton(parent, label, type, filePath) {
      const button = document.createElement("button");
      button.textContent = label;
      button.addEventListener("click", () => {
        vscode.postMessage({ type, path: filePath });
      });
      parent.appendChild(button);
    }
    function renderGeneratedPatchQueue(message) {
      generatedPatchQueue.textContent = "";
      if (!message.patches || message.patches.length === 0) {
        generatedPatchQueue.textContent = message.summary || "No generated patches.";
        return;
      }
      const heading = document.createElement("div");
      heading.textContent = message.summary || "Generated patches";
      generatedPatchQueue.appendChild(heading);
      const list = document.createElement("div");
      list.className = "queue-list";
      message.patches.forEach((patch) => {
        const item = document.createElement("div");
        item.className = "queue-item";
        const label = document.createElement("div");
        label.className = "queue-path";
        label.textContent = "[" + (patch.status || "pending") + "] #" + patch.id + " " + (patch.summary || "Generated patch");
        const source = document.createElement("div");
        source.className = "tool-meta";
        source.textContent = patch.source || "";
        const actions = document.createElement("div");
        actions.className = "queue-actions";
        addGeneratedPatchButton(actions, "Open", "openGeneratedPatch", patch.id);
        if ((patch.status || "pending") === "pending") {
          addGeneratedPatchButton(actions, "Apply", "applyGeneratedPatch", patch.id);
          addGeneratedPatchButton(actions, "Reject", "rejectGeneratedPatch", patch.id);
        }
        item.appendChild(label);
        item.appendChild(source);
        item.appendChild(actions);
        list.appendChild(item);
      });
      generatedPatchQueue.appendChild(list);
    }
    function addGeneratedPatchButton(parent, label, type, patchId) {
      const button = document.createElement("button");
      button.textContent = label;
      button.addEventListener("click", () => {
        vscode.postMessage({ type, id: patchId });
      });
      parent.appendChild(button);
    }
  </script>
</body>
</html>`;
}

function config() {
  return vscode.workspace.getConfiguration("deepseek");
}

function deepseekCommand() {
  return config().get("command", "deepseek").trim() || "deepseek";
}

function maxSelectionChars() {
  return config().get("maxSelectionChars", 6000);
}

function workspaceCwd() {
  const editor = vscode.window.activeTextEditor;
  if (editor) {
    const folder = vscode.workspace.getWorkspaceFolder(editor.document.uri);
    if (folder) {
      return folder.uri.fsPath;
    }
    if (editor.document.uri.scheme === "file") {
      return path.dirname(editor.document.uri.fsPath);
    }
  }

  const firstFolder = vscode.workspace.workspaceFolders?.[0];
  return firstFolder?.uri.fsPath;
}

function runInTerminal(command) {
  const terminal = vscode.window.createTerminal({
    name: "DeepseekCode",
    cwd: workspaceCwd(),
  });
  terminal.show(true);
  terminal.sendText(command);
}

function spawnDeepseek(args, cwd) {
  const command = `${deepseekCommand()} ${args.map(nativeShellQuote).join(" ")}`;
  return childProcess.spawn(command, [], {
    cwd,
    shell: true,
    env: process.env,
  });
}

async function quickAction() {
  const hasSelection = Boolean(
    vscode.window.activeTextEditor && !vscode.window.activeTextEditor.selection.isEmpty,
  );
  const picked = await vscode.window.showQuickPick(
    [
      {
        label: "$(layout-sidebar-right) Open Agent Panel",
        description: "Focus the DeepseekCode sidebar task panel",
        command: "deepseek.openPanel",
      },
      {
        label: "$(comment-discussion) Open Chat",
        description: "Start an interactive DeepseekCode session",
        command: "deepseek.openChat",
      },
      {
        label: "$(terminal) Run Task",
        description: "Prompt for a task in the current workspace",
        command: "deepseek.runTask",
      },
      {
        label: "$(symbol-method) Explain Selection",
        description: hasSelection
          ? "Send selected code as context"
          : "Uses the active file path as context",
        command: "deepseek.explainSelection",
      },
      {
        label: "$(warning) Explain Diagnostics",
        description: "Send active file problems as context",
        command: "deepseek.explainDiagnostics",
      },
      {
        label: "$(diff) Show Active Diff",
        description: "Open a review diff for the active file",
        command: "deepseek.showActiveDiff",
      },
      {
        label: "$(beaker) Run Benchmark",
        description: "Run the local benchmark suite",
        command: "deepseek.runBenchmark",
      },
      {
        label: "$(graph) Show Dogfood Report",
        description: "Show recent dogfood runs",
        command: "deepseek.showDogfoodReport",
      },
    ],
    {
      title: "DeepseekCode",
      placeHolder: workspaceCwd() || "No workspace folder open",
      ignoreFocusOut: true,
    },
  );
  if (picked) {
    await vscode.commands.executeCommand(picked.command);
  }
}

async function openPanel() {
  await vscode.commands.executeCommand("deepseek.panel.focus");
}

async function openChat() {
  runInTerminal(deepseekCommand());
}

async function runTask() {
  const task = await vscode.window.showInputBox({
    title: "DeepseekCode Task",
    prompt: "Task to run in the current workspace",
    ignoreFocusOut: true,
  });
  if (!task || !task.trim()) {
    return;
  }

  runInTerminal(`${deepseekCommand()} run ${shellQuote(await promptWithWorkbenchContext(task.trim()))}`);
}

async function explainSelection() {
  const prompt = await promptWithWorkbenchContext("Explain this code and point out correctness risks.");
  if (!prompt) {
    vscode.window.showInformationMessage("Open a file or select code before running this command.");
    return;
  }

  runInTerminal(`${deepseekCommand()} run ${shellQuote(prompt)}`);
}

async function explainDiagnostics() {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    vscode.window.showInformationMessage("Open a file before running this command.");
    return;
  }

  const diagnostics = vscode.languages.getDiagnostics(editor.document.uri);
  if (diagnostics.length === 0) {
    vscode.window.showInformationMessage("No VS Code diagnostics found for the active file.");
    return;
  }

  const filePath = relativeDocumentPath(editor.document) || editor.document.uri.fsPath;
  const renderedDiagnostics = diagnostics.slice(0, 20).map(formatDiagnostic).join("\n");
  const truncated = diagnostics.length > 20 ? `\n- truncated after 20 of ${diagnostics.length} diagnostics` : "";
  const task = [
    "Explain these VS Code diagnostics and suggest a minimal fix.",
    "",
    `File: ${filePath}`,
    "Diagnostics:",
    `${renderedDiagnostics}${truncated}`,
  ].join("\n");

  runInTerminal(`${deepseekCommand()} run ${shellQuote(task)}`);
}

async function showActiveDiff() {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.uri.scheme !== "file") {
    vscode.window.showInformationMessage("Open a file-backed editor before showing a diff.");
    return;
  }

  let gitRoot;
  try {
    gitRoot = await gitRootForDocument(editor.document);
  } catch (error) {
    vscode.window.showInformationMessage("No Git repository found for the active file.");
    return;
  }

  const relativePath = toGitPath(path.relative(gitRoot, editor.document.uri.fsPath));
  let baseContent;
  try {
    const result = await execFile("git", ["show", `HEAD:${relativePath}`], {
      cwd: gitRoot,
      maxBuffer: 10 * 1024 * 1024,
    });
    baseContent = result.stdout;
  } catch (error) {
    vscode.window.showInformationMessage("No HEAD version found for the active file.");
    return;
  }

  const language = editor.document.languageId;
  const baseDocument = await vscode.workspace.openTextDocument({ content: baseContent, language });
  const currentDocument = await vscode.workspace.openTextDocument({
    content: editor.document.getText(),
    language,
  });
  await vscode.commands.executeCommand(
    "vscode.diff",
    baseDocument.uri,
    currentDocument.uri,
    `DeepseekCode Diff: HEAD vs Editor - ${relativePath}`,
  );
}

async function runBenchmark() {
  runInTerminal(`${deepseekCommand()} benchmark`);
}

async function showDogfoodReport() {
  runInTerminal(`${deepseekCommand()} dogfood report --limit 10`);
}

async function promptWithWorkbenchContext(task) {
  const editor = vscode.window.activeTextEditor;
  if (!editor) {
    return task;
  }

  const relativePath = relativeDocumentPath(editor.document);
  const selectionText = selectedText(editor);
  const contextParts = [];

  if (relativePath) {
    contextParts.push(`File: ${relativePath}`);
  }
  if (editor.document.isDirty) {
    contextParts.push("Editor buffer has unsaved changes: true");
  }
  if (selectionText) {
    contextParts.push(`Selection:\n${selectionText}`);
  }
  const diagnostics = vscode.languages.getDiagnostics(editor.document.uri);
  if (diagnostics.length > 0) {
    const rendered = diagnostics.slice(0, 20).map(formatDiagnostic).join("\n");
    const truncated = diagnostics.length > 20
      ? `\n- truncated after 20 of ${diagnostics.length} diagnostics`
      : "";
    contextParts.push(`VS Code diagnostics:\n${rendered}${truncated}`);
  }
  const diff = await gitDiffSummaryForDocument(editor.document);
  if (diff) {
    contextParts.push(`Git diff summary:\n${diff}`);
  }

  if (contextParts.length === 0) {
    return task;
  }
  return `${task}\n\nVS Code context:\n${contextParts.join("\n\n")}`;
}

function relativeDocumentPath(document) {
  if (document.uri.scheme !== "file") {
    return undefined;
  }
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  if (!folder) {
    return document.uri.fsPath;
  }
  return path.relative(folder.uri.fsPath, document.uri.fsPath);
}

function selectedText(editor) {
  if (editor.selection.isEmpty) {
    return "";
  }
  const raw = editor.document.getText(editor.selection);
  const limit = maxSelectionChars();
  if (raw.length <= limit) {
    return raw;
  }
  return `${raw.slice(0, limit)}\n[truncated after ${limit} characters]`;
}

async function gitRootForDocument(document) {
  const folder = vscode.workspace.getWorkspaceFolder(document.uri);
  const cwd = folder ? folder.uri.fsPath : path.dirname(document.uri.fsPath);
  const result = await execFile("git", ["rev-parse", "--show-toplevel"], {
    cwd,
    maxBuffer: 1024 * 1024,
  });
  return result.stdout.trim();
}

async function gitDiffSummaryForDocument(document) {
  if (document.uri.scheme !== "file") {
    return "";
  }
  let gitRoot;
  try {
    gitRoot = await gitRootForDocument(document);
  } catch (error) {
    return "";
  }
  const relativePath = toGitPath(path.relative(gitRoot, document.uri.fsPath));
  const parts = [];
  try {
    const status = await execFile("git", ["status", "--short", "--", relativePath], {
      cwd: gitRoot,
      maxBuffer: 1024 * 1024,
    });
    if (status.stdout.trim()) {
      parts.push(status.stdout.trim());
    }
  } catch (error) {
    // Ignore git status failures; editor context should remain best-effort.
  }
  try {
    const diff = await execFile("git", ["diff", "--shortstat", "--", relativePath], {
      cwd: gitRoot,
      maxBuffer: 1024 * 1024,
    });
    if (diff.stdout.trim()) {
      parts.push(diff.stdout.trim());
    }
  } catch (error) {
    // Ignore git diff failures; editor context should remain best-effort.
  }
  return parts.join("\n");
}

async function activeFilePatchReview() {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.uri.scheme !== "file") {
    return { hasDiff: false, summary: "No file-backed active editor.", path: "", gitRoot: "" };
  }
  let gitRoot;
  try {
    gitRoot = await gitRootForDocument(editor.document);
  } catch (error) {
    return { hasDiff: false, summary: "No Git repository found for the active file.", path: "", gitRoot: "" };
  }
  const relativePath = toGitPath(path.relative(gitRoot, editor.document.uri.fsPath));
  let status = "";
  try {
    const result = await execFile("git", ["status", "--short", "--", relativePath], {
      cwd: gitRoot,
      maxBuffer: 1024 * 1024,
    });
    status = result.stdout.trim();
  } catch (error) {
    return { hasDiff: false, summary: `Could not inspect Git status: ${error.message}`, path: relativePath, gitRoot };
  }
  const untracked = status.startsWith("??");
  let shortstat = "";
  let diffPreview = "";
  if (!untracked) {
    try {
      const stat = await execFile("git", ["diff", "--shortstat", "--", relativePath], {
        cwd: gitRoot,
        maxBuffer: 1024 * 1024,
      });
      shortstat = stat.stdout.trim();
      let diff = await execFile("git", ["diff", "--", relativePath], {
        cwd: gitRoot,
        maxBuffer: 1024 * 1024,
      });
      if (!diff.stdout.trim() && status) {
        diff = await execFile("git", ["diff", "--cached", "--", relativePath], {
          cwd: gitRoot,
          maxBuffer: 1024 * 1024,
        });
      }
      diffPreview = clipPanelText(diff.stdout.trim());
    } catch (error) {
      diffPreview = `Could not render diff preview: ${error.message}`;
    }
  }
  const hasDiff = Boolean(status || shortstat || diffPreview);
  const summary = [
    relativePath ? `File: ${relativePath}` : "",
    status ? `Status: ${status}` : "",
    shortstat ? `Summary: ${shortstat}` : "",
    diffPreview,
  ].filter(Boolean).join("\n");
  return {
    hasDiff,
    summary: summary || "No active-file diff.",
    path: relativePath,
    gitRoot,
    untracked,
  };
}

async function workspacePatchQueue(reviewedPaths) {
  let gitRoot;
  try {
    gitRoot = await gitRootForWorkspace();
  } catch (error) {
    return {
      gitRoot: "",
      files: [],
      summary: "No Git repository found for the workspace.",
    };
  }
  let statusOutput = "";
  try {
    const status = await execFile("git", ["-c", "core.quotepath=false", "status", "--short"], {
      cwd: gitRoot,
      maxBuffer: 2 * 1024 * 1024,
    });
    statusOutput = status.stdout.replace(/\s+$/g, "");
  } catch (error) {
    return {
      gitRoot,
      files: [],
      summary: `Could not inspect workspace changes: ${error.message}`,
    };
  }
  const files = statusOutput
    .split(/\r?\n/)
    .map(parseGitStatusLine)
    .filter(Boolean)
    .map((file) => ({
      ...file,
      reviewed: reviewedPaths.has(reviewedPathKey(gitRoot, file.path)),
    }));
  let shortstat = "";
  try {
    const diff = await execFile("git", ["diff", "--shortstat"], {
      cwd: gitRoot,
      maxBuffer: 1024 * 1024,
    });
    shortstat = diff.stdout.trim();
  } catch (error) {
    // Keep the queue useful even when shortstat fails.
  }
  const reviewedCount = files.filter((file) => file.reviewed).length;
  const summary = files.length === 0
    ? "No workspace changes."
    : [
        `${files.length} changed file${files.length === 1 ? "" : "s"}`,
        reviewedCount > 0 ? `${reviewedCount} reviewed` : "",
        shortstat,
      ].filter(Boolean).join(" · ");
  return { gitRoot, files, summary };
}

function parseGitStatusLine(line) {
  if (!line || line.length < 4) {
    return undefined;
  }
  const status = line.slice(0, 2);
  let filePath = line.slice(3).trim();
  if (!filePath) {
    return undefined;
  }
  if (filePath.includes(" -> ")) {
    filePath = filePath.split(" -> ").pop();
  }
  return {
    status,
    path: filePath,
    untracked: status === "??",
    deleted: status.includes("D"),
  };
}

async function gitRootForWorkspace() {
  const cwd = workspaceCwd();
  if (!cwd) {
    throw new Error("No workspace folder open");
  }
  const result = await execFile("git", ["rev-parse", "--show-toplevel"], {
    cwd,
    maxBuffer: 1024 * 1024,
  });
  return result.stdout.trim();
}

function reviewedPathKey(gitRoot, filePath) {
  return `${gitRoot}:${filePath}`;
}

async function showWorkspaceFileDiff(gitRoot, item) {
  if (item.untracked) {
    const uri = vscode.Uri.file(path.join(gitRoot, item.path));
    const document = await vscode.workspace.openTextDocument(uri);
    await vscode.window.showTextDocument(document);
    return;
  }
  const relativePath = item.path;
  let baseContent = "";
  try {
    const result = await execFile("git", ["show", `HEAD:${relativePath}`], {
      cwd: gitRoot,
      maxBuffer: 10 * 1024 * 1024,
    });
    baseContent = result.stdout;
  } catch (error) {
    baseContent = "";
  }
  const language = languageFromPath(relativePath);
  const baseDocument = await vscode.workspace.openTextDocument({ content: baseContent, language });
  let currentContent = "";
  if (!item.deleted) {
    try {
      currentContent = await fsReadFile(path.join(gitRoot, relativePath));
    } catch (error) {
      currentContent = "";
    }
  }
  const currentDocument = await vscode.workspace.openTextDocument({
    content: currentContent,
    language,
  });
  await vscode.commands.executeCommand(
    "vscode.diff",
    baseDocument.uri,
    currentDocument.uri,
    `DeepseekCode Diff: HEAD vs Workspace - ${relativePath}`,
  );
}

async function showGeneratedPatchDiff(gitRoot, patchArtifact) {
  if (!patchArtifact.files || patchArtifact.files.length !== 1) {
    return false;
  }
  const relativePath = patchArtifact.files[0];
  const language = languageFromPath(relativePath);
  const fullPath = path.join(gitRoot, relativePath);
  let beforeContent = "";
  let beforeExists = true;
  try {
    beforeContent = await fsReadFile(fullPath);
  } catch (error) {
    beforeExists = false;
  }
  const afterContent = await renderGeneratedPatchAfterContent(
    relativePath,
    beforeContent,
    beforeExists,
    patchArtifact.patch,
  );
  const baseDocument = await vscode.workspace.openTextDocument({
    content: beforeContent,
    language,
  });
  const proposedDocument = await vscode.workspace.openTextDocument({
    content: afterContent,
    language,
  });
  await vscode.commands.executeCommand(
    "vscode.diff",
    baseDocument.uri,
    proposedDocument.uri,
    `DeepseekCode Generated Patch #${patchArtifact.id}: ${relativePath}`,
  );
  return true;
}

async function renderGeneratedPatchAfterContent(relativePath, beforeContent, beforeExists, patch) {
  const tempRoot = await fs.promises.mkdtemp(path.join(os.tmpdir(), "dscode-vscode-patch-"));
  try {
    const tempPath = path.join(tempRoot, relativePath);
    if (beforeExists && !patchCreatesFile(patch, relativePath)) {
      await fs.promises.mkdir(path.dirname(tempPath), { recursive: true });
      await fs.promises.writeFile(tempPath, beforeContent, "utf8");
    }
    await runGitApplyChecked(tempRoot, patch);
    try {
      return await fs.promises.readFile(tempPath, "utf8");
    } catch (error) {
      return "";
    }
  } finally {
    await fs.promises.rm(tempRoot, { recursive: true, force: true });
  }
}

function patchCreatesFile(patch, relativePath) {
  const targetVariants = new Set([relativePath, `b/${relativePath}`]);
  const lines = String(patch || "").split(/\r?\n/);
  for (let index = 0; index < lines.length - 1; index += 1) {
    if (lines[index].startsWith("--- ") && normalizePatchHeaderPath(lines[index].slice(4).trim()) === "/dev/null") {
      const nextPath = normalizePatchHeaderPath(lines[index + 1].replace(/^\+\+\+\s+/, "").trim());
      return targetVariants.has(nextPath);
    }
  }
  return false;
}

function languageFromPath(filePath) {
  const ext = path.extname(filePath).replace(/^\./, "").toLowerCase();
  const mapping = {
    js: "javascript",
    jsx: "javascriptreact",
    ts: "typescript",
    tsx: "typescriptreact",
    rs: "rust",
    py: "python",
    md: "markdown",
    json: "json",
    toml: "toml",
    yml: "yaml",
    yaml: "yaml",
  };
  return mapping[ext] || ext || "plaintext";
}

function fsReadFile(filePath) {
  return new Promise((resolve, reject) => {
    fs.readFile(filePath, "utf8", (error, data) => {
      if (error) {
        reject(error);
      } else {
        resolve(data);
      }
    });
  });
}

async function runGitApplyChecked(cwd, patch) {
  const candidates = [
    {
      name: "default",
      check: ["apply", "--check", "-"],
      apply: ["apply", "-"],
    },
    {
      name: "p0",
      check: ["apply", "--check", "-p0", "-"],
      apply: ["apply", "-p0", "-"],
    },
  ];
  const errors = [];
  for (const candidate of candidates) {
    try {
      await runGitApply(candidate.check, cwd, patch);
      await runGitApply(candidate.apply, cwd, patch);
      return candidate.name;
    } catch (error) {
      errors.push(`${candidate.name}: ${error.message}`);
    }
  }
  throw new Error(errors.join("\n"));
}

function runGitApply(args, cwd, patch) {
  return new Promise((resolve, reject) => {
    const child = childProcess.spawn("git", args, {
      cwd,
      env: process.env,
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    child.on("error", reject);
    child.on("close", (code, signal) => {
      if (code === 0) {
        resolve({ stdout, stderr });
        return;
      }
      reject(new Error([
        `git ${args.join(" ")} failed`,
        code !== null ? `exit ${code}` : "",
        signal ? `signal ${signal}` : "",
        stderr.trim() || stdout.trim(),
      ].filter(Boolean).join(": ")));
    });
    child.stdin.end(patch);
  });
}

function generatedPatchesFromExecEvent(event) {
  if (!event || typeof event !== "object") {
    return [];
  }
  const artifacts = [];
  if (event.type === "tool_call" && event.tool === "apply_patch") {
    const patch = patchTextFromToolInput(event.input);
    if (patch) {
      artifacts.push({
        patch,
        source: "apply_patch tool call",
        status: "captured",
      });
    }
  }
  if (event.type === "assistant_final" && event.message) {
    for (const patch of extractUnifiedDiffs(event.message)) {
      artifacts.push({ patch, source: "assistant final" });
    }
  }
  if (event.type === "tool_result" && event.output) {
    for (const patch of extractUnifiedDiffs(event.output)) {
      artifacts.push({ patch, source: `${event.tool || "tool"} result` });
    }
  }
  return artifacts;
}

function patchTextFromToolInput(input) {
  if (!input) {
    return "";
  }
  if (typeof input === "object" && typeof input.patch === "string") {
    return isUnifiedPatch(input.patch) ? input.patch : "";
  }
  if (typeof input !== "string") {
    return "";
  }
  try {
    const parsed = JSON.parse(input);
    if (parsed && typeof parsed.patch === "string") {
      return isUnifiedPatch(parsed.patch) ? parsed.patch : "";
    }
  } catch (error) {
    // Fall through to treating the string itself as patch text.
  }
  return isUnifiedPatch(input) ? input : "";
}

function extractUnifiedDiffs(text) {
  if (!text) {
    return [];
  }
  const patches = [];
  const seen = new Set();
  const addPatch = (candidate) => {
    const normalized = normalizeGeneratedPatch(candidate);
    if (!normalized || seen.has(normalized) || !isUnifiedPatch(normalized)) {
      return;
    }
    seen.add(normalized);
    patches.push(normalized);
  };

  const source = String(text);
  const fencePattern = /```[^\n`]*\n([\s\S]*?)```/g;
  let match;
  while ((match = fencePattern.exec(source)) !== null) {
    addPatch(match[1]);
  }

  for (const patch of extractRawUnifiedDiffs(source)) {
    addPatch(patch);
  }
  return patches;
}

function extractRawUnifiedDiffs(text) {
  const lines = String(text).replace(/\r\n/g, "\n").split("\n");
  const patches = [];
  let index = 0;
  while (index < lines.length) {
    if (!isPatchStartAt(lines, index)) {
      index += 1;
      continue;
    }
    const start = index;
    let sawHunk = false;
    index += 1;
    while (index < lines.length) {
      const line = lines[index];
      if (/^@@\s/.test(line)) {
        sawHunk = true;
        index += 1;
        continue;
      }
      if (isPatchStructureLine(line) || isPatchBodyLine(line)) {
        index += 1;
        continue;
      }
      if (line === "" && !sawHunk) {
        index += 1;
        continue;
      }
      break;
    }
    const candidate = lines.slice(start, index).join("\n");
    if (isUnifiedPatch(candidate)) {
      patches.push(candidate);
    }
  }
  return patches;
}

function isPatchStartAt(lines, index) {
  const line = lines[index] || "";
  if (line.startsWith("diff --git ")) {
    return true;
  }
  return line.startsWith("--- ") && (lines[index + 1] || "").startsWith("+++ ");
}

function isPatchStructureLine(line) {
  return line.startsWith("diff --git ")
    || line.startsWith("index ")
    || line.startsWith("new file mode ")
    || line.startsWith("deleted file mode ")
    || line.startsWith("old mode ")
    || line.startsWith("new mode ")
    || line.startsWith("similarity index ")
    || line.startsWith("dissimilarity index ")
    || line.startsWith("rename from ")
    || line.startsWith("rename to ")
    || line.startsWith("--- ")
    || line.startsWith("+++ ")
    || line.startsWith("\\ No newline at end of file");
}

function isPatchBodyLine(line) {
  return line.startsWith("+")
    || line.startsWith("-")
    || line.startsWith(" ");
}

function normalizeGeneratedPatch(patch) {
  const normalized = String(patch || "").replace(/\r\n/g, "\n").trim();
  return normalized ? `${normalized}\n` : "";
}

function isUnifiedPatch(patch) {
  const text = String(patch || "");
  return /(^|\n)---\s+\S/.test(text)
    && /(^|\n)\+\+\+\s+\S/.test(text)
    && /(^|\n)@@\s/.test(text);
}

function generatedPatchFiles(patch) {
  const files = [];
  const seen = new Set();
  for (const line of String(patch || "").split(/\r?\n/)) {
    if (!line.startsWith("+++ ")) {
      continue;
    }
    const filePath = normalizePatchHeaderPath(line.slice(4).trim());
    if (!filePath || filePath === "/dev/null" || seen.has(filePath)) {
      continue;
    }
    seen.add(filePath);
    files.push(filePath);
  }
  return files;
}

function normalizePatchHeaderPath(value) {
  const trimmed = value.split(/\t/)[0].trim().replace(/^"|"$/g, "");
  if (trimmed.startsWith("a/") || trimmed.startsWith("b/")) {
    return trimmed.slice(2);
  }
  return trimmed;
}

function generatedPatchSummary(patch) {
  const files = generatedPatchFiles(patch);
  const hunks = (String(patch || "").match(/^@@\s/gm) || []).length;
  const fileText = files.length === 0
    ? "unknown files"
    : files.length === 1
      ? files[0]
      : `${files.length} files`;
  return [
    fileText,
    `${hunks} hunk${hunks === 1 ? "" : "s"}`,
    `${String(patch || "").length} chars`,
  ].join(" · ");
}

function generatedPatchQueueSummary(patches) {
  if (!patches || patches.length === 0) {
    return "No generated patches.";
  }
  const pending = patches.filter((patch) => patch.status === "pending").length;
  const captured = patches.filter((patch) => patch.status === "captured").length;
  const applied = patches.filter((patch) => patch.status === "applied").length;
  const rejected = patches.filter((patch) => patch.status === "rejected").length;
  return [
    `${patches.length} generated patch${patches.length === 1 ? "" : "es"}`,
    pending > 0 ? `${pending} pending` : "",
    captured > 0 ? `${captured} captured` : "",
    applied > 0 ? `${applied} applied` : "",
    rejected > 0 ? `${rejected} rejected` : "",
  ].filter(Boolean).join(" · ");
}

function toGitPath(value) {
  return value.split(path.sep).join("/");
}

function formatDiagnostic(diagnostic) {
  const severity = ["error", "warning", "info", "hint"][diagnostic.severity] || "diagnostic";
  const start = diagnostic.range.start;
  return [
    `- ${severity}${formatDiagnosticCode(diagnostic.code)}`,
    `L${start.line + 1}:C${start.character + 1}:`,
    oneLine(diagnostic.message),
  ].join(" ");
}

function formatDiagnosticCode(code) {
  if (code === undefined || code === null) {
    return "";
  }
  if (typeof code === "object" && "value" in code) {
    return ` [${code.value}]`;
  }
  return ` [${code}]`;
}

function oneLine(value) {
  return String(value).replace(/\s+/g, " ").trim();
}

function shellQuote(value) {
  return `'${String(value).replace(/'/g, `'\\''`)}'`;
}

function nativeShellQuote(value) {
  if (process.platform === "win32") {
    return `"${String(value).replace(/"/g, '\\"')}"`;
  }
  return shellQuote(value);
}

function drainJsonLines(buffer, onLine) {
  const lines = buffer.split(/\r?\n/);
  const rest = lines.pop() || "";
  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed) {
      onLine(trimmed);
    }
  }
  return rest;
}

function postExecJsonLine(view, line, onEvent) {
  let event;
  try {
    event = JSON.parse(line);
  } catch (error) {
    view.webview.postMessage({ type: "log", level: "stdout", text: `${line}\n` });
    return;
  }
  if (onEvent) {
    onEvent(event);
  }
  switch (event.type) {
    case "assistant_delta":
      view.webview.postMessage({ type: "assistantDelta", text: event.delta || "" });
      break;
    case "assistant_reasoning_delta":
      view.webview.postMessage({ type: "reasoningDelta", text: event.delta || "" });
      break;
    case "assistant_final":
      view.webview.postMessage({ type: "final", message: event.message || "" });
      break;
    case "tool_call":
      view.webview.postMessage({
        type: "toolEvent",
        tool: event.tool || "tool",
        detail: clipPanelText(JSON.stringify(event.input || {})),
      });
      break;
    case "permission_request":
      view.webview.postMessage({
        type: "toolEvent",
        tool: event.tool || "permission",
        detail: `${event.kind || "permission"} ${event.target || ""}`.trim(),
      });
      break;
    case "tool_result":
      view.webview.postMessage({
        type: "toolEvent",
        tool: event.tool || "tool",
        detail: `${event.status || "done"} ${clipPanelText(event.output || "")}`.trim(),
      });
      break;
    case "session_started":
      view.webview.postMessage({ type: "log", level: "session", text: "Session started.\n" });
      break;
    case "error":
      view.webview.postMessage({ type: "log", level: "error", text: `${event.message || "error"}\n` });
      break;
    default:
      view.webview.postMessage({ type: "log", level: event.type || "event", text: `${line}\n` });
      break;
  }
}

function clipPanelText(value) {
  const text = String(value).replace(/\s+$/g, "");
  const limit = 4000;
  if (text.length <= limit) {
    return text;
  }
  return `${text.slice(0, limit)}\n[truncated after ${limit} characters]`;
}

function nonce() {
  let value = "";
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  for (let index = 0; index < 32; index += 1) {
    value += chars.charAt(Math.floor(Math.random() * chars.length));
  }
  return value;
}

module.exports = {
  activate,
  deactivate,
  shellQuote,
  drainJsonLines,
  clipPanelText,
  parseGitStatusLine,
  extractUnifiedDiffs,
  generatedPatchesFromExecEvent,
  generatedPatchFiles,
  generatedPatchSummary,
  generatedPatchQueueSummary,
};
