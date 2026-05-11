#!/usr/bin/env node
"use strict";

const {
  chmodSync,
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
} = require("node:fs");
const { join, resolve } = require("node:path");

function usage() {
  console.error(
    "usage: node npm/scripts/stage-platform-package.js --platform <name> --binary <path>",
  );
  process.exit(2);
}

function parseArgs(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 2) {
    const key = argv[index];
    const value = argv[index + 1];
    if (!key || !key.startsWith("--") || !value) {
      usage();
    }
    args[key.slice(2)] = value;
  }
  if (!args.platform || !args.binary) {
    usage();
  }
  return args;
}

const args = parseArgs(process.argv.slice(2));
const repoRoot = resolve(__dirname, "..", "..");
const packageDir = join(repoRoot, "npm", "platforms", args.platform);
const packageJson = join(packageDir, "package.json");
const rootLicense = join(repoRoot, "LICENSE");
const binary = resolve(args.binary);

if (!existsSync(packageJson)) {
  console.error(`unknown npm platform package: ${args.platform}`);
  process.exit(1);
}
if (!existsSync(binary)) {
  console.error(`release binary does not exist: ${binary}`);
  process.exit(1);
}

const metadata = JSON.parse(readFileSync(packageJson, "utf8"));
const executable = metadata.os && metadata.os.includes("win32") ? "deepseek.exe" : "deepseek";
const binDir = join(packageDir, "bin");
const destination = join(binDir, executable);

mkdirSync(binDir, { recursive: true });
copyFileSync(rootLicense, join(packageDir, "LICENSE"));
copyFileSync(binary, destination);
if (executable === "deepseek") {
  chmodSync(destination, 0o755);
}

console.log(`staged ${binary} into ${metadata.name}:${destination}`);
