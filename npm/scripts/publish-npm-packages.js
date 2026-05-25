#!/usr/bin/env node
"use strict";

const { execFileSync } = require("node:child_process");
const { existsSync, readdirSync, readFileSync } = require("node:fs");
const { resolve } = require("node:path");

function usage() {
  console.error("usage: node scripts/publish-npm-packages.js --dist <dir> --root <npm-root>");
  process.exit(2);
}

function parseArgs(argv) {
  const args = { dist: null, root: null };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--help" || arg === "-h") {
      usage();
    }
    if (arg === "--dist") {
      args.dist = argv[index + 1];
      index += 1;
      continue;
    }
    if (arg === "--root") {
      args.root = argv[index + 1];
      index += 1;
      continue;
    }
    usage();
  }
  if (!args.dist || !args.root) {
    usage();
  }
  return {
    dist: resolve(args.dist),
    root: resolve(args.root),
  };
}

function readPackageJson(path) {
  return JSON.parse(readFileSync(path, "utf8"));
}

function readTarballPackageJson(path) {
  const output = execFileSync("tar", ["-xOf", path, "package/package.json"], {
    encoding: "utf8",
  });
  return JSON.parse(output);
}

function packageExists(name, version) {
  const result = execFileSync("npm", ["view", `${name}@${version}`, "version"], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  return result.trim() === version;
}

function safePackageExists(name, version) {
  try {
    return packageExists(name, version);
  } catch {
    return false;
  }
}

function publishPackage(spec, name, version) {
  if (safePackageExists(name, version)) {
    console.log(`npm package already published: ${name}@${version}`);
    return;
  }
  console.log(`publishing npm package: ${name}@${version}`);
  execFileSync("npm", ["publish", spec, "--access", "public"], {
    stdio: "inherit",
  });
}

const args = parseArgs(process.argv.slice(2));
if (!existsSync(args.dist)) {
  throw new Error(`missing npm dist directory: ${args.dist}`);
}
if (!existsSync(args.root)) {
  throw new Error(`missing npm root directory: ${args.root}`);
}

const tarballs = readdirSync(args.dist)
  .filter((entry) => entry.endsWith(".tgz"))
  .sort()
  .map((entry) => resolve(args.dist, entry));

if (tarballs.length === 0) {
  throw new Error(`no npm tarballs found in ${args.dist}`);
}

for (const tarball of tarballs) {
  const packageJson = readTarballPackageJson(tarball);
  publishPackage(tarball, packageJson.name, packageJson.version);
}

const rootPackage = readPackageJson(resolve(args.root, "package.json"));
publishPackage(args.root, rootPackage.name, rootPackage.version);
