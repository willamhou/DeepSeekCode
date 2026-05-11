#!/usr/bin/env node
"use strict";

const { existsSync, readFileSync, statSync } = require("node:fs");
const { join, resolve } = require("node:path");
const { spawnSync } = require("node:child_process");

function usage() {
  console.error(
    "usage: node npm/scripts/verify-platform-package.js --platform <name>",
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
  if (!args.platform) {
    usage();
  }
  return args;
}

const args = parseArgs(process.argv.slice(2));
const repoRoot = resolve(__dirname, "..", "..");
const packageDir = join(repoRoot, "npm", "platforms", args.platform);
const packageJson = join(packageDir, "package.json");

if (!existsSync(packageJson)) {
  console.error(`unknown npm platform package: ${args.platform}`);
  process.exit(1);
}

const metadata = JSON.parse(readFileSync(packageJson, "utf8"));
const executable = metadata.os && metadata.os.includes("win32") ? "deepseek.exe" : "deepseek";
const binary = join(packageDir, "bin", executable);

if (!existsSync(binary)) {
  console.error(`platform package binary is missing: ${binary}`);
  process.exit(1);
}

if (executable === "deepseek" && (statSync(binary).mode & 0o111) === 0) {
  console.error(`platform package binary is not executable: ${binary}`);
  process.exit(1);
}

const result = spawnSync(binary, ["version"], { encoding: "utf8" });
if (result.error && result.status === null) {
  console.error(result.error.message);
  process.exit(1);
}
if (result.status !== 0) {
  process.stderr.write(result.stderr || "");
  process.exit(result.status || 1);
}
if (!result.stdout.includes("deepseek ")) {
  console.error(`unexpected platform package binary output: ${result.stdout.trim()}`);
  process.exit(1);
}

console.log(`platform package ok: ${metadata.name} ${result.stdout.trim()}`);
