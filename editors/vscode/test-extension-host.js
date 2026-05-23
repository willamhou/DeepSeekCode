const childProcess = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");

const extensionPath = __dirname;

function main() {
  const codeBin = findVsCodeBinary();
  if (!codeBin) {
    const message = "Skipping VS Code extension-host smoke: set VSCODE_BIN or install code/codium.";
    if (process.env.DSCODE_REQUIRE_VSCODE === "1") {
      console.error(message);
      process.exit(1);
    }
    console.log(message);
    return;
  }

  const tempRoot = fs.mkdtempSync(path.join(os.tmpdir(), "dscode-vscode-host-"));
  const workspaceDir = path.join(tempRoot, "workspace");
  const vscodeDir = path.join(workspaceDir, ".vscode");
  const srcDir = path.join(workspaceDir, "src");
  fs.mkdirSync(vscodeDir, { recursive: true });
  fs.mkdirSync(srcDir, { recursive: true });
  fs.writeFileSync(path.join(srcDir, "example.js"), 'const value = "old";\n');

  const mockBin = path.join(tempRoot, process.platform === "win32" ? "mock-deepseek.cmd" : "mock-deepseek.js");
  writeMockDeepseek(mockBin);
  fs.writeFileSync(
    path.join(vscodeDir, "settings.json"),
    JSON.stringify({
      "deepseek.command": process.platform === "win32" ? mockBin : `node ${mockBin}`,
      "deepseek.maxSelectionChars": 6000,
    }, null, 2),
  );

  const args = [
    "--extensionDevelopmentPath", extensionPath,
    "--extensionTestsPath", path.join(extensionPath, "test-extension-host-runner.js"),
    "--user-data-dir", path.join(tempRoot, "user-data"),
    "--extensions-dir", path.join(tempRoot, "extensions"),
    "--disable-extensions",
    "--skip-welcome",
    workspaceDir,
  ];

  const result = childProcess.spawnSync(codeBin, args, {
    env: {
      ...process.env,
      DSCODE_VSCODE_SMOKE_WORKSPACE: workspaceDir,
      DSCODE_VSCODE_SMOKE_MOCK_BIN: mockBin,
    },
    stdio: "inherit",
  });
  fs.rmSync(tempRoot, { recursive: true, force: true });
  if (result.error) {
    throw result.error;
  }
  process.exit(result.status === null ? 1 : result.status);
}

function findVsCodeBinary() {
  const candidates = [
    process.env.VSCODE_BIN,
    "code",
    "code-insiders",
    "codium",
  ].filter(Boolean);
  for (const candidate of candidates) {
    const result = childProcess.spawnSync(candidate, ["--version"], {
      stdio: "ignore",
    });
    if (!result.error && result.status === 0) {
      return candidate;
    }
  }
  return "";
}

function writeMockDeepseek(mockBin) {
  if (process.platform === "win32") {
    fs.writeFileSync(mockBin, "@echo off\r\nnode %~dp0mock-deepseek-js.js %*\r\n");
    fs.writeFileSync(path.join(path.dirname(mockBin), "mock-deepseek-js.js"), mockDeepseekJs());
    return;
  }
  fs.writeFileSync(mockBin, `#!/usr/bin/env node\n${mockDeepseekJs()}`);
  fs.chmodSync(mockBin, 0o755);
}

function mockDeepseekJs() {
  const patch = [
    "```diff",
    "--- src/example.js",
    "+++ src/example.js",
    "@@ -1 +1 @@",
    "-const value = \"old\";",
    "+const value = \"new\";",
    "```",
  ].join("\\n");
  return [
    "const events = [",
    "  { type: 'session_started' },",
    "  { type: 'assistant_delta', delta: 'Preparing patch.\\n' },",
    `  { type: 'assistant_final', message: ${JSON.stringify(patch)} },`,
    "];",
    "for (const event of events) console.log(JSON.stringify(event));",
  ].join("\n");
}

main();
