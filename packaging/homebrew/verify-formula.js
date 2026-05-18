#!/usr/bin/env node
"use strict";

const { existsSync, readFileSync } = require("node:fs");
const { resolve } = require("node:path");
const { spawnSync } = require("node:child_process");

const repoRoot = resolve(__dirname, "..", "..");
const defaultFormula = resolve(repoRoot, "packaging", "homebrew", "deepseek.rb");

function usage() {
  console.error(
    [
      "usage: node packaging/homebrew/verify-formula.js [--formula <path>] [--release]",
      "",
      "Checks the DeepSeekCode Homebrew formula template without requiring Homebrew.",
      "--release rejects placeholder zero SHA-256 values.",
    ].join("\n"),
  );
  process.exit(2);
}

function parseArgs(argv) {
  const args = { formula: defaultFormula, release: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help" || arg === "-h") {
      usage();
    }
    if (arg === "--release") {
      args.release = true;
      continue;
    }
    if (arg === "--formula") {
      const value = argv[index + 1];
      if (!value) {
        usage();
      }
      args.formula = resolve(value);
      index += 1;
      continue;
    }
    usage();
  }
  return args;
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

function readText(path) {
  if (!existsSync(path)) {
    fail(`missing file: ${path}`);
  }
  return readFileSync(path, "utf8");
}

function stringValue(text, key) {
  return text.match(new RegExp(`^\\s*${key}\\s*(?:=\\s*)?"([^"]+)"`, "m"))?.[1] || null;
}

function cargoVersion() {
  const cargoToml = readText(resolve(repoRoot, "Cargo.toml"));
  const version = stringValue(cargoToml, "version");
  if (!version) {
    fail("Cargo.toml is missing package version");
  }
  return version;
}

function count(text, pattern) {
  return (text.match(pattern) || []).length;
}

function validateRubySyntax(formula) {
  const result = spawnSync("ruby", ["-c", formula], { encoding: "utf8" });
  if (result.error && result.error.code === "ENOENT") {
    return "ruby syntax skipped: ruby not found";
  }
  if (result.error) {
    fail(result.error.message);
  }
  if (result.status !== 0) {
    process.stderr.write(result.stderr || result.stdout || "");
    fail(`ruby syntax check failed for ${formula}`);
  }
  return "ruby syntax ok";
}

const args = parseArgs(process.argv.slice(2));
const formula = readText(args.formula);
const version = cargoVersion();
const expectedTag = `v${version}`;
const failures = [];

if (!formula.includes("class Deepseek < Formula")) {
  failures.push("formula class must be `Deepseek < Formula`");
}
if (stringValue(formula, "desc") !== "DeepSeek-first terminal code agent") {
  failures.push("formula desc is missing or unexpected");
}
if (stringValue(formula, "homepage") !== "https://github.com/willamhou/DeepSeekCode") {
  failures.push("formula homepage is missing or unexpected");
}
if (stringValue(formula, "version") !== version) {
  failures.push(`formula version must match Cargo.toml ${version}`);
}

for (const platform of ["macos-arm64", "macos-x64", "linux-x64"]) {
  const url = `https://github.com/willamhou/DeepSeekCode/releases/download/${expectedTag}/deepseek-${platform}.tar.gz`;
  if (!formula.includes(`url "${url}"`)) {
    failures.push(`missing release URL for ${platform}`);
  }
}

if (!formula.includes('bin.install binary => "deepseek"')) {
  failures.push("install block must install the binary as deepseek");
}
if (!formula.includes('shell_output("#{bin}/deepseek version")')) {
  failures.push("test block must run `deepseek version`");
}
if (!formula.includes('system "#{bin}/deepseek", "doctor", "--json"')) {
  failures.push("test block must run `deepseek doctor --json`");
}

const shas = [...formula.matchAll(/^\s*sha256\s+"([0-9a-fA-F]{64})"/gm)].map((match) =>
  match[1].toLowerCase(),
);
if (shas.length !== 3) {
  failures.push(`expected 3 sha256 entries, found ${shas.length}`);
}
const placeholderShas = shas.filter((sha) => /^0{64}$/.test(sha)).length;
if (args.release && placeholderShas > 0) {
  failures.push("release formula must not contain placeholder zero SHA-256 values");
}
if (count(formula, /on_macos do/g) !== 1 || count(formula, /on_linux do/g) !== 1) {
  failures.push("formula must contain one macOS block and one Linux block");
}

if (failures.length > 0) {
  fail(`Homebrew formula verification failed:\n${failures.join("\n")}`);
}

const rubyStatus = validateRubySyntax(args.formula);
const shaStatus =
  placeholderShas > 0
    ? `template placeholders accepted (${placeholderShas})`
    : "release SHA-256 values present";

console.log(
  [
    "Homebrew formula ok",
    `formula: ${args.formula}`,
    `version: ${version}`,
    `sha256: ${shaStatus}`,
    rubyStatus,
  ].join("\n"),
);
