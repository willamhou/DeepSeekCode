const assert = require("assert");
const Module = require("module");

const registeredCommands = [];
const registeredViews = [];
const registeredTrees = [];

const disposable = { dispose() {} };
const fakeVscode = {
  StatusBarAlignment: { Left: 1 },
  ThemeIcon: class ThemeIcon {
    constructor(id) {
      this.id = id;
    }
  },
  TreeItem: class TreeItem {
    constructor(label, collapsibleState) {
      this.label = label;
      this.collapsibleState = collapsibleState;
    }
  },
  TreeItemCollapsibleState: { None: 0 },
  window: {
    activeTextEditor: undefined,
    createStatusBarItem() {
      return { show() {}, dispose() {} };
    },
    registerTreeDataProvider(id, provider) {
      registeredTrees.push({ id, provider });
      return disposable;
    },
    registerWebviewViewProvider(id, provider) {
      registeredViews.push({ id, provider });
      return disposable;
    },
    createTerminal() {
      return { show() {}, sendText() {} };
    },
    showInformationMessage() {},
    showWarningMessage() {
      return undefined;
    },
    showQuickPick() {
      return undefined;
    },
    showInputBox() {
      return undefined;
    },
  },
  commands: {
    registerCommand(command, callback) {
      registeredCommands.push({ command, callback });
      return disposable;
    },
    executeCommand() {},
  },
  workspace: {
    workspaceFolders: undefined,
    getConfiguration() {
      return {
        get(_key, fallback) {
          return fallback;
        },
      };
    },
    getWorkspaceFolder() {
      return undefined;
    },
    openTextDocument() {
      return undefined;
    },
  },
  languages: {
    getDiagnostics() {
      return [];
    },
  },
};

const originalLoad = Module._load;
Module._load = function patchedLoad(request, parent, isMain) {
  if (request === "vscode") {
    return fakeVscode;
  }
  return originalLoad.call(this, request, parent, isMain);
};

const extension = require("./extension");

const context = { subscriptions: [] };
extension.activate(context);

assert(registeredCommands.some((entry) => entry.command === "deepseek.quickAction"));
assert(registeredCommands.some((entry) => entry.command === "deepseek.runTask"));
assert(registeredCommands.some((entry) => entry.command === "deepseek.explainDiagnostics"));
assert(registeredTrees.some((entry) => entry.id === "deepseek.actions"));
const panel = registeredViews.find((entry) => entry.id === "deepseek.panel");
assert(panel, "panel provider should be registered");

let receivedMessageHandler;
const view = {
  webview: {
    options: undefined,
    html: "",
    onDidReceiveMessage(callback) {
      receivedMessageHandler = callback;
    },
    postMessage() {},
  },
};
panel.provider.resolveWebviewView(view);
assert.strictEqual(view.webview.options.enableScripts, true);
assert(view.webview.html.includes("assistant"));
assert(view.webview.html.includes("toolEvent"));
assert(view.webview.html.includes("reviewDiff"));
assert(view.webview.html.includes("refreshPatchQueue"));
assert(view.webview.html.includes("generatedPatchQueue"));
assert(view.webview.html.includes("Generated Patches"));
assert(view.webview.html.includes("Resume Latest"));
assert(view.webview.html.includes("validationCommand"));
assert.strictEqual(typeof receivedMessageHandler, "function");

const lines = [];
const rest = extension.drainJsonLines('{"type":"a"}\n{"type":"b"}', (line) => {
  lines.push(line);
});
assert.deepStrictEqual(lines, ['{"type":"a"}']);
assert.strictEqual(rest, '{"type":"b"}');
assert(extension.clipPanelText("x".repeat(5000)).includes("[truncated after 4000 characters]"));
assert.deepStrictEqual(extension.parseGitStatusLine(" M editors/vscode/extension.js"), {
  status: " M",
  path: "editors/vscode/extension.js",
  untracked: false,
  deleted: false,
});
assert.deepStrictEqual(extension.parseGitStatusLine("?? new-file.txt"), {
  status: "??",
  path: "new-file.txt",
  untracked: true,
  deleted: false,
});
assert.deepStrictEqual(extension.parseGitStatusLine("R  old.txt -> new.txt"), {
  status: "R ",
  path: "new.txt",
  untracked: false,
  deleted: false,
});

const generatedDiff = [
  "```diff",
  "--- src/a.js",
  "+++ src/a.js",
  "@@ -1 +1 @@",
  "-old",
  "+new",
  "```",
].join("\n");
const extracted = extension.extractUnifiedDiffs(`Patch:\n${generatedDiff}`);
assert.strictEqual(extracted.length, 1);
assert(extracted[0].includes("--- src/a.js"));
assert.deepStrictEqual(extension.generatedPatchFiles(extracted[0]), ["src/a.js"]);
assert.strictEqual(extension.generatedPatchSummary(extracted[0]), "src/a.js · 1 hunk · 48 chars");

const artifacts = extension.generatedPatchesFromExecEvent({
  type: "assistant_final",
  message: generatedDiff,
});
assert.strictEqual(artifacts.length, 1);
assert.strictEqual(artifacts[0].source, "assistant final");

const toolArtifacts = extension.generatedPatchesFromExecEvent({
  type: "tool_call",
  tool: "apply_patch",
  input: {
    patch: "--- src/b.js\n+++ src/b.js\n@@ -1 +1 @@\n-a\n+b\n",
  },
});
assert.strictEqual(toolArtifacts.length, 1);
assert.strictEqual(toolArtifacts[0].status, "captured");
assert.strictEqual(
  extension.generatedPatchQueueSummary([
    { status: "pending" },
    { status: "captured" },
    { status: "rejected" },
  ]),
  "3 generated patches · 1 pending · 1 captured · 1 rejected",
);

extension.deactivate();
