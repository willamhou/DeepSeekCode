#!/usr/bin/env node
"use strict";

const fs = require("node:fs");

const usage = `Usage: docs/demo/render-tui-ux-evidence-svg.js [--self-test] [--out <svg>] <evidence.log>

Render a reviewed TUI UX evidence log into a static terminal-style SVG.
`;

function escapeXml(value) {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function wrapLine(line, width = 92) {
  if (line.length <= width) {
    return [line];
  }
  const chunks = [];
  let remaining = line;
  while (remaining.length > width) {
    let splitAt = remaining.lastIndexOf(" ", width);
    if (splitAt < Math.floor(width * 0.55)) {
      splitAt = width;
    }
    chunks.push(remaining.slice(0, splitAt));
    remaining = remaining.slice(splitAt).trimStart();
  }
  if (remaining) {
    chunks.push(remaining);
  }
  return chunks;
}

function importantLines(log) {
  const lines = log.split(/\r?\n/);
  const keep = [];
  const wanted = [
    /^# DeepSeekCode TUI UX evidence$/,
    /^workspace:/,
    /^binary:/,
    /^\$ /,
    /^case:/,
    /^expect:/,
    /^observed:/,
    /^status: ok$/,
  ];
  for (const line of lines) {
    if (wanted.some((pattern) => pattern.test(line))) {
      keep.push(line);
    }
  }
  if (!keep.some((line) => line === "status: ok")) {
    throw new Error("evidence log is missing status: ok");
  }
  for (const required of [
    "Next setup: Choose provider",
    "Next setup: Store API key",
    "Next setup: Inspect trust",
    "Next setup: Review network",
  ]) {
    if (!log.includes(required)) {
      throw new Error(`evidence log is missing ${required}`);
    }
  }
  return keep.flatMap((line) => wrapLine(line));
}

function renderSvg(log) {
  const lines = importantLines(log);
  const charWidth = 8.4;
  const lineHeight = 19;
  const paddingX = 26;
  const paddingTop = 54;
  const paddingBottom = 28;
  const maxChars = Math.max(...lines.map((line) => line.length), 70);
  const width = Math.max(860, Math.ceil(maxChars * charWidth + paddingX * 2));
  const height = paddingTop + paddingBottom + lines.length * lineHeight;
  const text = lines
    .map((line, index) => {
      const y = paddingTop + index * lineHeight;
      let color = "#dbe3ef";
      if (line.startsWith("$ ")) color = "#8bd5ff";
      if (line.startsWith("expect:")) color = "#f4d35e";
      if (line.startsWith("observed:")) color = "#a7f3d0";
      if (line === "status: ok") color = "#73e2a7";
      return `<text x="${paddingX}" y="${y}" fill="${color}">${escapeXml(line)}</text>`;
    })
    .join("\n");

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">
  <rect width="${width}" height="${height}" rx="8" fill="#111827"/>
  <circle cx="24" cy="24" r="6" fill="#ff5f57"/>
  <circle cx="44" cy="24" r="6" fill="#ffbd2e"/>
  <circle cx="64" cy="24" r="6" fill="#28c840"/>
  <text x="88" y="29" fill="#9ca3af" font-family="Inter, ui-sans-serif, system-ui" font-size="14">DeepSeekCode TUI UX evidence</text>
  <g font-family="SFMono-Regular, Consolas, 'Liberation Mono', Menlo, monospace" font-size="14">
${text}
  </g>
</svg>
`;
}

function selfTest() {
  const log = `# DeepSeekCode TUI UX evidence
workspace: /tmp/deepseek-code-tui-ux-evidence
binary: target/debug/deepseek
$ deepseek tui --once # fresh repo
case: fresh repo without provider/model/auth
expect: Setup guide -> Next setup: Choose provider -> Jump: /setup provider
observed: Setup guide | Next setup: Choose provider (project config missing) | Jump: /setup provider
$ deepseek tui --once # provider/model configured
case: provider/model configured without API key
expect: Next setup: Store API key -> Jump: /setup auth DSC_TUI_KEY
observed: Next setup: Store API key (DSC_TUI_KEY missing) | Jump: /setup auth DSC_TUI_KEY
$ deepseek tui --once # API key available
case: API key present, trust not reviewed
expect: Next setup: Inspect trust -> Jump: /setup trust
observed: Next setup: Inspect trust (inspect permissions) | Jump: /setup trust
$ deepseek tui --once # trust reviewed
case: trust reviewed, network policy missing
expect: Next setup: Review network -> Jump: /setup network
observed: Next setup: Review network (default allow; review policy) | Jump: /setup network
status: ok
`;
  const svg = renderSvg(log);
  if (!svg.includes("<svg") || !svg.includes("status: ok")) {
    throw new Error("self-test SVG render failed");
  }
}

function main(argv) {
  let out = "docs/demo/deepseek-code-tui-ux-evidence.svg";
  let input = null;
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--help" || arg === "-h") {
      process.stdout.write(usage);
      return;
    }
    if (arg === "--self-test") {
      selfTest();
      process.stdout.write("render-tui-ux-evidence-svg self-test ok\n");
      return;
    }
    if (arg === "--out") {
      out = argv[i + 1];
      i += 1;
      continue;
    }
    if (arg.startsWith("--")) {
      throw new Error(`unknown argument: ${arg}`);
    }
    input = arg;
  }
  if (!input) {
    process.stderr.write(usage);
    process.exit(2);
  }
  const log = fs.readFileSync(input, "utf8");
  fs.writeFileSync(out, renderSvg(log));
}

try {
  main(process.argv.slice(2));
} catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exit(1);
}
