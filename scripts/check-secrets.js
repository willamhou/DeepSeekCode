#!/usr/bin/env node
"use strict";

const { readdirSync, readFileSync } = require("node:fs");
const { join } = require("node:path");

const SECRET_PATTERNS = [
  {
    name: "api-key-token",
    pattern: /\bsk-[A-Za-z0-9_-]{20,}\b/g,
  },
];

const TEXT_EXTENSIONS = new Set([
  "",
  ".cjs",
  ".js",
  ".json",
  ".lock",
  ".md",
  ".rb",
  ".rs",
  ".sh",
  ".toml",
  ".txt",
  ".yml",
  ".yaml",
]);

const SKIP_DIRS = new Set([
  ".agents",
  ".dscode",
  ".git",
  ".idea",
  ".vscode",
  "node_modules",
  "target",
]);

const SKIP_PATH_PATTERNS = [
  /^\.env(?:\.|$)/,
  /^docs\/demo\/deepseek-code-model-demo-.*\.log$/,
  /^npm\/platforms\/[^/]+\/bin\//,
  /\.key$/,
];

function extension(path) {
  const slash = path.lastIndexOf("/");
  const dot = path.lastIndexOf(".");
  if (dot <= slash) {
    return "";
  }
  return path.slice(dot);
}

function shouldSkipPath(path) {
  return SKIP_PATH_PATTERNS.some((pattern) => pattern.test(path));
}

function scanFiles(root, prefix = "") {
  const files = [];
  for (const entry of readdirSync(root, { withFileTypes: true })) {
    const relative = prefix ? `${prefix}/${entry.name}` : entry.name;
    const fullPath = join(root, entry.name);
    if (entry.isDirectory()) {
      if (!SKIP_DIRS.has(entry.name) && !shouldSkipPath(relative)) {
        files.push(...scanFiles(fullPath, relative));
      }
      continue;
    }
    if (!entry.isFile()) {
      continue;
    }
    if (!shouldSkipPath(relative)) {
      files.push(relative);
    }
  }
  return files;
}

function lineAndColumn(text, offset) {
  let line = 1;
  let column = 1;
  for (let index = 0; index < offset; index += 1) {
    if (text.charCodeAt(index) === 10) {
      line += 1;
      column = 1;
    } else {
      column += 1;
    }
  }
  return { line, column };
}

function lineTextAt(text, offset) {
  const start = text.lastIndexOf("\n", offset - 1) + 1;
  const end = text.indexOf("\n", offset);
  return text.slice(start, end === -1 ? text.length : end);
}

function mask(value) {
  if (value.length <= 10) {
    return "<redacted>";
  }
  return `${value.slice(0, 6)}...${value.slice(-4)}`;
}

const findings = [];

for (const file of scanFiles(process.cwd())) {
  if (!TEXT_EXTENSIONS.has(extension(file))) {
    continue;
  }
  let content;
  try {
    content = readFileSync(file, "utf8");
  } catch {
    continue;
  }
  if (content.includes("\u0000")) {
    continue;
  }
  for (const { name, pattern } of SECRET_PATTERNS) {
    pattern.lastIndex = 0;
    for (let match = pattern.exec(content); match; match = pattern.exec(content)) {
      if (lineTextAt(content, match.index).includes("secret-scan: allow")) {
        continue;
      }
      const location = lineAndColumn(content, match.index);
      findings.push({
        file,
        line: location.line,
        column: location.column,
        name,
        value: mask(match[0]),
      });
    }
  }
}

if (findings.length > 0) {
  console.error("Potential secrets found in tracked files:");
  for (const finding of findings) {
    console.error(
      `${finding.file}:${finding.line}:${finding.column} ${finding.name} ${finding.value}`,
    );
  }
  process.exit(1);
}

console.log("secret scan ok");
