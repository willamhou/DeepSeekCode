#!/usr/bin/env node
"use strict";

const {
  chmodSync,
  mkdirSync,
  rmSync,
  writeFileSync,
} = require("node:fs");
const { join } = require("node:path");
const { spawnSync } = require("node:child_process");

const TARGETS = {
  "darwin:arm64": "macos-arm64",
  "darwin:x64": "macos-x64",
  "linux:x64": "linux-x64",
};

const suffix = TARGETS[`${process.platform}:${process.arch}`];
if (!suffix || process.platform === "win32") {
  console.log("optional package resolution smoke skipped on this platform");
  process.exit(0);
}

const packageRoot = join(__dirname, "..");
const packageName = `@deepseek-code/cli-${suffix}`;
const platformPackage = join(packageRoot, "node_modules", "@deepseek-code", `cli-${suffix}`);
const executable = join(platformPackage, "bin", "deepseek");

rmSync(platformPackage, { recursive: true, force: true });
mkdirSync(join(platformPackage, "bin"), { recursive: true });
writeFileSync(
  join(platformPackage, "package.json"),
  JSON.stringify({ name: packageName, version: "0.0.0" }, null, 2),
);
writeFileSync(executable, "#!/bin/sh\necho wrapper-platform-ok\n");
chmodSync(executable, 0o755);

try {
  const env = { ...process.env };
  delete env.DEEPSEEK_BINARY;
  const result = spawnSync(process.execPath, [join(packageRoot, "bin", "deepseek.js"), "version"], {
    encoding: "utf8",
    env,
  });
  if (result.status !== 0) {
    process.stderr.write(result.stderr || "");
    process.stderr.write(result.stdout || "");
    process.exit(result.status || 1);
  }
  if (result.stdout.trim() !== "wrapper-platform-ok") {
    process.stderr.write(`unexpected wrapper output: ${result.stdout}`);
    process.exit(1);
  }
} finally {
  rmSync(platformPackage, { recursive: true, force: true });
}
