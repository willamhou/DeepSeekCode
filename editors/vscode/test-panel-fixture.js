const assert = require("assert");
const childProcess = require("child_process");
const fs = require("fs");
const Module = require("module");
const os = require("os");
const path = require("path");

const disposable = { dispose() {} };
const executedCommands = [];
let activeTextEditor;
let workspaceDir;
let mockDeepseekPath;

function makeUri(filePath) {
  return {
    scheme: "file",
    fsPath: filePath,
  };
}

function makeDocument(filePath, languageId = "javascript") {
  return {
    uri: makeUri(filePath),
    languageId,
    isDirty: false,
    getText() {
      return fs.readFileSync(filePath, "utf8");
    },
  };
}

function makeUntitledDocument(content, languageId = "plaintext") {
  return {
    uri: {
      scheme: "untitled",
      fsPath: "",
    },
    languageId,
    isDirty: false,
    getText() {
      return content;
    },
  };
}

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
  Uri: {
    file: makeUri,
  },
  window: {
    get activeTextEditor() {
      return activeTextEditor;
    },
    set activeTextEditor(value) {
      activeTextEditor = value;
    },
    createStatusBarItem() {
      return { show() {}, dispose() {} };
    },
    registerTreeDataProvider() {
      return disposable;
    },
    registerWebviewViewProvider() {
      return disposable;
    },
    createTerminal() {
      return { show() {}, sendText() {} };
    },
    showInformationMessage() {},
    showWarningMessage(_message, _options, choice) {
      return choice;
    },
    showQuickPick() {
      return undefined;
    },
    showInputBox() {
      return undefined;
    },
    showTextDocument(document) {
      if (document.uri.scheme === "file") {
        activeTextEditor = makeEditor(document);
      }
      return Promise.resolve(activeTextEditor);
    },
  },
  commands: {
    registerCommand() {
      return disposable;
    },
    executeCommand(command, ...args) {
      executedCommands.push({ command, args });
      return Promise.resolve();
    },
  },
  workspace: {
    get workspaceFolders() {
      return [{ uri: makeUri(workspaceDir) }];
    },
    getConfiguration() {
      return {
        get(key, fallback) {
          if (key === "command") {
            return `${quoteForShell(process.execPath)} ${quoteForShell(mockDeepseekPath)}`;
          }
          return fallback;
        },
      };
    },
    getWorkspaceFolder(uri) {
      if (uri?.scheme === "file" && uri.fsPath.startsWith(workspaceDir)) {
        return { uri: makeUri(workspaceDir) };
      }
      return undefined;
    },
    openTextDocument(input) {
      if (typeof input === "string") {
        return Promise.resolve(makeDocument(input));
      }
      if (input?.scheme === "file") {
        return Promise.resolve(makeDocument(input.fsPath));
      }
      if (typeof input?.content === "string") {
        return Promise.resolve(makeUntitledDocument(input.content, input.language));
      }
      throw new Error(`unsupported openTextDocument input: ${String(input)}`);
    },
  },
  languages: {
    getDiagnostics() {
      return [
        {
          severity: 0,
          code: "fixture",
          message: "Expected value to be new",
          range: {
            start: {
              line: 0,
              character: 14,
            },
          },
        },
      ];
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

async function main() {
  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "dscode-panel-fixture-"));
  try {
    workspaceDir = path.join(tempRoot, "workspace");
    const srcDir = path.join(workspaceDir, "src");
    fs.mkdirSync(srcDir, { recursive: true });
    fs.writeFileSync(path.join(srcDir, "example.js"), 'const value = "old";\n');
    initGitRepo(workspaceDir);

    mockDeepseekPath = path.join(tempRoot, "mock-deepseek.js");
    writeMockDeepseek(mockDeepseekPath);
    const validationPath = path.join(tempRoot, "validate.js");
    fs.writeFileSync(validationPath, [
      "const fs = require('fs');",
      "const text = fs.readFileSync('src/example.js', 'utf8');",
      "if (!text.includes('\"new\"')) process.exit(1);",
      "console.log('fixture validation passed');",
    ].join("\n"));

    const extension = require("./extension");
    const api = extension.activate({ subscriptions: [] });
    const provider = api.createTestPanelProvider();
    const messages = [];
    let messageHandler;
    const view = {
      webview: {
        options: undefined,
        html: "",
        onDidReceiveMessage(callback) {
          messageHandler = callback;
          return disposable;
        },
        postMessage(message) {
          messages.push(message);
          return Promise.resolve(true);
        },
      },
    };

    const activePath = path.join(workspaceDir, "src", "example.js");
    activeTextEditor = makeEditor(makeDocument(activePath));
    provider.resolveWebviewView(view);
    assert.strictEqual(view.webview.options.enableScripts, true);
    assert.strictEqual(typeof messageHandler, "function");

    await messageHandler({ type: "runTask", task: "Fix the active diagnostic." });
    await waitFor(() => messages.some((message) => message.type === "runEnded"), 8000);
    assert(
      capturedPrompt().includes("VS Code diagnostics")
        && capturedPrompt().includes("Expected value to be new"),
      "panel prompt should include VS Code diagnostics",
    );

    const patchQueue = latestMessage(messages, "generatedPatchQueue", (message) => message.patches?.length > 0);
    assert(patchQueue, "generated patch queue should contain a pending artifact");
    const patchId = patchQueue.patches[0].id;
    assert.strictEqual(patchQueue.patches[0].status, "pending");

    await messageHandler({ type: "openGeneratedPatch", id: patchId });
    assert(
      executedCommands.some((entry) => entry.command === "vscode.diff"),
      `opening a single-file generated patch should invoke vscode.diff; commands=${JSON.stringify(executedCommands)} logs=${JSON.stringify(messages.filter((message) => message.type === "log"))} queue=${JSON.stringify(patchQueue)}`,
    );

    await messageHandler({ type: "applyGeneratedPatch", id: patchId });
    await waitFor(() => fs.readFileSync(activePath, "utf8").includes('"new"'), 8000);
    const appliedQueue = latestMessage(messages, "generatedPatchQueue", (message) => message.patches?.[0]?.status === "applied");
    assert(appliedQueue, "generated patch should be marked applied");

    const validationCommand = `${quoteForShell(process.execPath)} ${quoteForShell(validationPath)}`;
    await messageHandler({ type: "validate", command: validationCommand });
    await waitFor(() => latestMessage(messages, "validationEnded"), 8000);
    const validation = latestMessage(messages, "validationEnded");
    assert.strictEqual(validation.ok, true);

    const workspaceQueue = latestMessage(messages, "patchQueue", (message) => message.files?.length > 0);
    assert(workspaceQueue, "workspace patch queue should refresh after applying generated patch");
    assert(
      workspaceQueue.files.some((file) => file.path === "src/example.js"),
      `workspace queue should include src/example.js: ${JSON.stringify(workspaceQueue)}`,
    );
  } finally {
    fs.rmSync(tempRoot, { recursive: true, force: true });
    Module._load = originalLoad;
  }
}

function makeEditor(document) {
  return {
    document,
    selection: {
      isEmpty: true,
    },
  };
}

function initGitRepo(cwd) {
  run("git", ["init"], cwd);
  run("git", ["config", "user.email", "fixture@example.invalid"], cwd);
  run("git", ["config", "user.name", "Fixture"], cwd);
  run("git", ["add", "src/example.js"], cwd);
  run("git", ["commit", "-m", "initial"], cwd);
}

function run(command, args, cwd) {
  const result = childProcess.spawnSync(command, args, {
    cwd,
    encoding: "utf8",
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed: ${result.stderr || result.stdout}`);
  }
}

function writeMockDeepseek(filePath) {
  const script = [
    "const fs = require('fs');",
    "const prompt = process.argv.slice(2).join('\\n');",
    "fs.writeFileSync(process.env.DSCODE_PANEL_FIXTURE_PROMPT, prompt);",
    "if (!prompt.includes('VS Code diagnostics')) process.exit(12);",
    "const patch = [",
    "  '```diff',",
    "  '--- src/example.js',",
    "  '+++ src/example.js',",
    "  '@@ -1 +1 @@',",
    "  '-const value = \"old\";',",
    "  '+const value = \"new\";',",
    "  '```',",
    "].join('\\n');",
    "console.log(JSON.stringify({ type: 'session_started' }));",
    "console.log(JSON.stringify({ type: 'assistant_delta', delta: 'diagnostic patch ready\\n' }));",
    "console.log(JSON.stringify({ type: 'assistant_final', message: patch }));",
  ].join("\n");
  fs.writeFileSync(filePath, script);
}

function capturedPrompt() {
  const promptPath = process.env.DSCODE_PANEL_FIXTURE_PROMPT;
  return fs.readFileSync(promptPath, "utf8");
}

function latestMessage(messages, type, predicate = () => true) {
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = messages[index];
    if (message.type === type && predicate(message)) {
      return message;
    }
  }
  return undefined;
}

async function waitFor(predicate, timeoutMs) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    if (predicate()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error("Timed out waiting for panel fixture condition");
}

function quoteForShell(value) {
  if (process.platform === "win32") {
    return `"${String(value).replace(/"/g, '\\"')}"`;
  }
  return `'${String(value).replace(/'/g, `'\\''`)}'`;
}

process.env.DSCODE_PANEL_FIXTURE_PROMPT = path.join(os.tmpdir(), `dscode-panel-prompt-${process.pid}.txt`);

main().finally(() => {
  try {
    fs.rmSync(process.env.DSCODE_PANEL_FIXTURE_PROMPT, { force: true });
  } catch (error) {
    // Best-effort temp cleanup.
  }
});
