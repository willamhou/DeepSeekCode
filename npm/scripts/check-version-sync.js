#!/usr/bin/env node

const { existsSync, readFileSync } = require("fs");
const { join } = require("path");

const repoRoot = join(__dirname, "..", "..");
const npmRoot = join(repoRoot, "npm");
const platformDirs = ["linux-x64", "macos-arm64", "macos-x64", "windows-x64"];
const failures = [];

function readText(path) {
  return readFileSync(join(repoRoot, path), "utf8");
}

function readJson(path) {
  return JSON.parse(readText(path));
}

function fail(message) {
  failures.push(message);
}

const cargoToml = readText("Cargo.toml");
const cargoVersion = cargoToml.match(/^\s*version\s*=\s*"([^"]+)"/m)?.[1];
if (!cargoVersion) {
  fail("Cargo.toml package version was not found");
}

const rootPackage = readJson("npm/package.json");
if (cargoVersion && rootPackage.version !== cargoVersion) {
  fail(`npm/package.json version ${rootPackage.version} does not match Cargo.toml ${cargoVersion}`);
}
if (rootPackage.license !== "SEE LICENSE IN LICENSE") {
  fail(`npm/package.json license ${rootPackage.license} should be SEE LICENSE IN LICENSE`);
}
if (!existsSync(join(npmRoot, "LICENSE"))) {
  fail("npm/LICENSE is missing");
}

for (const platform of platformDirs) {
  const packageJson = readJson(`npm/platforms/${platform}/package.json`);
  const optionalVersion = rootPackage.optionalDependencies?.[packageJson.name];
  const licensePath = join(npmRoot, "platforms", platform, "LICENSE");

  if (cargoVersion && packageJson.version !== cargoVersion) {
    fail(`${packageJson.name} version ${packageJson.version} does not match Cargo.toml ${cargoVersion}`);
  }
  if (packageJson.license !== "SEE LICENSE IN LICENSE") {
    fail(`${packageJson.name} license ${packageJson.license} should be SEE LICENSE IN LICENSE`);
  }
  if (!existsSync(licensePath)) {
    fail(`${packageJson.name} LICENSE file is missing`);
  }

  if (cargoVersion && optionalVersion !== cargoVersion) {
    fail(
      `npm/package.json optionalDependency ${packageJson.name}=${optionalVersion ?? "<missing>"} does not match ${cargoVersion}`,
    );
  }
}

const homebrewFormula = readText("packaging/homebrew/deepseek.rb");
const homebrewVersion = homebrewFormula.match(/^\s*version\s+"([^"]+)"/m)?.[1];
if (cargoVersion && homebrewVersion !== cargoVersion) {
  fail(`Homebrew formula version ${homebrewVersion ?? "<missing>"} does not match Cargo.toml ${cargoVersion}`);
}

for (const artifact of ["deepseek-linux-x64", "deepseek-macos-x64", "deepseek-macos-arm64"]) {
  const expected = `/releases/download/v${cargoVersion}/${artifact}.tar.gz`;
  if (cargoVersion && !homebrewFormula.includes(expected)) {
    fail(`Homebrew formula is missing release URL suffix ${expected}`);
  }
}

if (failures.length > 0) {
  console.error("Version sync check failed:");
  for (const failure of failures) {
    console.error(`- ${failure}`);
  }
  process.exit(1);
}

console.log(`version sync ok: ${cargoVersion}`);
