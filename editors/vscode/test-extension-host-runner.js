const assert = require("assert");
const path = require("path");
const vscode = require("vscode");

async function run() {
  const workspaceDir = process.env.DSCODE_VSCODE_SMOKE_WORKSPACE;
  assert(workspaceDir, "DSCODE_VSCODE_SMOKE_WORKSPACE must be set");

  const extension = vscode.extensions.getExtension("deepseekcode.deepseek-code");
  assert(extension, "DeepseekCode extension should be available in the extension host");
  const api = await extension.activate();
  assert(api && typeof api.createTestPanelProvider === "function", "test panel API should be exposed");

  const document = await vscode.workspace.openTextDocument(path.join(workspaceDir, "src", "example.js"));
  await vscode.window.showTextDocument(document);

  const messages = [];
  let messageHandler;
  const provider = api.createTestPanelProvider();
  const view = {
    webview: {
      options: undefined,
      html: "",
      onDidReceiveMessage(callback) {
        messageHandler = callback;
        return { dispose() {} };
      },
      postMessage(message) {
        messages.push(message);
        return Promise.resolve(true);
      },
    },
  };

  provider.resolveWebviewView(view);
  assert.strictEqual(view.webview.options.enableScripts, true);
  assert(view.webview.html.includes("Generated Patches"));
  assert.strictEqual(typeof messageHandler, "function");

  await messageHandler({ type: "runTask", task: "produce a generated patch" });
  await waitFor(() => messages.some((message) => message.type === "runEnded"), 8000);

  assert(messages.some((message) => message.type === "assistantDelta"));
  const generatedQueue = [...messages].reverse().find((message) => message.type === "generatedPatchQueue");
  assert(generatedQueue, "generated patch queue should be posted");
  assert.strictEqual(generatedQueue.patches.length, 1);
  assert.strictEqual(generatedQueue.patches[0].status, "pending");
  assert(generatedQueue.patches[0].summary.includes("src/example.js"));
}

async function waitFor(predicate, timeoutMs) {
  const started = Date.now();
  while (Date.now() - started < timeoutMs) {
    if (predicate()) {
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error("Timed out waiting for extension-host smoke condition");
}

module.exports = { run };
