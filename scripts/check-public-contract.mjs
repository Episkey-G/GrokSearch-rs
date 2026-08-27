#!/usr/bin/env node

import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const failures = [];
let passed = 0;
// Derived from the run() calls below so adding a check cannot desync the
// summary line the way a hand-maintained total would.
let total = 0;

function fail(check, message) {
  failures.push(`[${check}] ${message}`);
}

function run(check, callback) {
  total += 1;
  const before = failures.length;
  try {
    callback();
  } catch (error) {
    fail(check, error instanceof Error ? error.message : String(error));
  }
  if (failures.length === before) passed += 1;
}

function read(relativePath) {
  try {
    return readFileSync(join(root, relativePath), "utf8");
  } catch (error) {
    throw new Error(`cannot read ${relativePath}: ${error.message}`);
  }
}

function readJson(relativePath) {
  try {
    return JSON.parse(read(relativePath));
  } catch (error) {
    throw new Error(`cannot parse ${relativePath} as JSON: ${error.message}`);
  }
}

function packageSection(cargoToml) {
  const match = /^\[package\]\s*$/m.exec(cargoToml);
  if (!match) throw new Error("Cargo.toml is missing a [package] section");
  const tail = cargoToml.slice(match.index + match[0].length);
  const nextSection = /^\s*\[[^\]]+\]\s*$/m.exec(tail);
  return nextSection ? tail.slice(0, nextSection.index) : tail;
}

function tomlString(section, key, file = "Cargo.toml") {
  const matches = [...section.matchAll(new RegExp(`^\\s*${key}\\s*=\\s*[\"']([^\"']+)[\"']\\s*$`, "gm"))];
  if (matches.length !== 1) {
    throw new Error(`${file} must contain exactly one string ${key} field in [package] (found ${matches.length})`);
  }
  return matches[0][1];
}

function normalizedRepository(raw) {
  if (typeof raw !== "string" || raw.trim() === "") {
    throw new Error("repository URL is missing or is not a string");
  }
  let value = raw.trim().replace(/^git\+/, "");
  const scp = /^git@([^:]+):(.+)$/.exec(value);
  if (scp) value = `https://${scp[1]}/${scp[2]}`;
  value = value.replace(/^ssh:\/\/git@/i, "https://");

  let parsed;
  try {
    parsed = new URL(value);
  } catch {
    throw new Error(`repository URL is not valid: ${raw}`);
  }
  const path = parsed.pathname.replace(/\/+$/, "").replace(/\.git$/i, "");
  return `${parsed.hostname.toLowerCase()}${path.toLowerCase()}`;
}

function extractRustFunction(source, name, file) {
  const signature = new RegExp(`(?:pub(?:\\([^)]*\\))?\\s+)?(?:async\\s+)?fn\\s+${name}\\s*\\([^)]*\\)[^{]*\\{`, "m");
  const match = signature.exec(source);
  if (!match) throw new Error(`${file}: could not find fn ${name}() or its opening brace`);
  const opening = match.index + match[0].lastIndexOf("{");
  let depth = 0;
  let state = "code";
  let blockCommentDepth = 0;

  for (let index = opening; index < source.length; index += 1) {
    const char = source[index];
    const next = source[index + 1];
    if (state === "line-comment") {
      if (char === "\n") state = "code";
      continue;
    }
    if (state === "block-comment") {
      if (char === "/" && next === "*") {
        blockCommentDepth += 1;
        index += 1;
      } else if (char === "*" && next === "/") {
        blockCommentDepth -= 1;
        index += 1;
        if (blockCommentDepth === 0) state = "code";
      }
      continue;
    }
    if (state === "string") {
      if (char === "\\") index += 1;
      else if (char === '"') state = "code";
      continue;
    }
    if (char === "/" && next === "/") {
      state = "line-comment";
      index += 1;
    } else if (char === "/" && next === "*") {
      state = "block-comment";
      blockCommentDepth = 1;
      index += 1;
    } else if (char === '"') {
      state = "string";
    } else if (char === "{") {
      depth += 1;
    } else if (char === "}") {
      depth -= 1;
      if (depth === 0) return source.slice(match.index, index + 1);
    }
  }
  throw new Error(`${file}: fn ${name}() has no balanced closing brace`);
}

function unique(values) {
  return [...new Set(values)];
}

function compareSets(check, actual, expected, actualLabel, expectedLabel) {
  const actualSet = new Set(actual);
  const expectedSet = new Set(expected);
  const missing = [...expectedSet].filter((value) => !actualSet.has(value)).sort();
  const extra = [...actualSet].filter((value) => !expectedSet.has(value)).sort();
  if (missing.length || extra.length) {
    fail(
      check,
      `${actualLabel} differs from ${expectedLabel}; missing=[${missing.join(", ")}], extra=[${extra.join(", ")}]`,
    );
  }
}

run("versions", () => {
  const cargoVersion = tomlString(packageSection(read("Cargo.toml")), "version");
  const mainPath = "npm/grok-search-rs/package.json";
  const main = readJson(mainPath);
  if (main.version !== cargoVersion) {
    fail("versions", `${mainPath} version=${main.version ?? "<missing>"}, expected ${cargoVersion} from Cargo.toml`);
  }

  const platformPaths = [
    "npm/platforms/darwin-universal/package.json",
    "npm/platforms/linux-arm64/package.json",
    "npm/platforms/linux-x64/package.json",
    "npm/platforms/win32-arm64/package.json",
    "npm/platforms/win32-x64/package.json",
  ];
  const platformNames = [];
  for (const path of platformPaths) {
    let pkg;
    try {
      pkg = readJson(path);
    } catch (error) {
      fail("versions", error.message);
      continue;
    }
    if (typeof pkg.name !== "string" || pkg.name === "") {
      fail("versions", `${path} is missing package name`);
      continue;
    }
    platformNames.push(pkg.name);
    if (pkg.version !== cargoVersion) {
      fail("versions", `${path} version=${pkg.version ?? "<missing>"}, expected ${cargoVersion}`);
    }
  }

  if (!main.optionalDependencies || typeof main.optionalDependencies !== "object" || Array.isArray(main.optionalDependencies)) {
    fail("versions", `${mainPath} must contain an optionalDependencies object`);
    return;
  }
  for (const name of platformNames) {
    if (!(name in main.optionalDependencies)) {
      fail("versions", `${mainPath} optionalDependencies is missing ${name}`);
    }
  }
  for (const [name, version] of Object.entries(main.optionalDependencies)) {
    if (version !== cargoVersion) {
      fail("versions", `${mainPath} optionalDependencies[${name}]=${version}, expected ${cargoVersion}`);
    }
  }
  if (unique(platformNames).length !== platformNames.length) {
    fail("versions", "platform package names must be unique");
  }
});

run("server.json", () => {
  if (!existsSync(join(root, "server.json"))) return;
  const cargoVersion = tomlString(packageSection(read("Cargo.toml")), "version");
  const npm = readJson("npm/grok-search-rs/package.json");
  const server = readJson("server.json");
  if (server.version !== cargoVersion) {
    fail("server.json", `server.json version=${server.version ?? "<missing>"}, expected ${cargoVersion}`);
  }
  if (typeof npm.mcpName !== "string" || npm.mcpName === "") {
    fail("server.json", "npm/grok-search-rs/package.json must define mcpName when server.json exists");
  } else if (server.name !== npm.mcpName) {
    fail("server.json", `server.json name=${server.name ?? "<missing>"}, expected npm mcpName=${npm.mcpName}`);
  }
});

run("MCP tools", () => {
  const toolsFunction = extractRustFunction(read("src/mcp.rs"), "tools_list", "src/mcp.rs");
  const sourceTools = [...toolsFunction.matchAll(/"name"\s*:\s*"([A-Za-z0-9_-]+)"/g)].map((match) => match[1]);
  if (sourceTools.length === 0) throw new Error("src/mcp.rs: tools_list() contains no parsable tool names");
  if (unique(sourceTools).length !== sourceTools.length) {
    fail("MCP tools", `src/mcp.rs tools_list() contains duplicate names: ${sourceTools.join(", ")}`);
  }

  const readme = read("README.md");
  const heading = /^##\s+MCP Tools\s*$/m.exec(readme);
  if (!heading) throw new Error('README.md is missing the "## MCP Tools" section');
  const tail = readme.slice(heading.index + heading[0].length);
  const nextHeading = /^##\s+/m.exec(tail);
  const section = nextHeading ? tail.slice(0, nextHeading.index) : tail;
  const documentedTools = [...section.matchAll(/^\|\s*`([^`]+)`\s*\|/gm)].map((match) => match[1].trim());
  if (documentedTools.length === 0) {
    throw new Error("README.md MCP Tools table has no backticked tool names in its first column");
  }
  if (unique(documentedTools).length !== documentedTools.length) {
    fail("MCP tools", `README.md MCP Tools table contains duplicate names: ${documentedTools.join(", ")}`);
  }
  compareSets("MCP tools", documentedTools, sourceTools, "README.md MCP Tools", "src/mcp.rs tools_list()");
});

run("response budget", () => {
  const config = read("src/config.rs");
  const sourceMatch = /response_max_chars\s*:\s*usize_value\s*\(\s*&map\s*,\s*"GROK_SEARCH_RESPONSE_MAX_CHARS"\s*,\s*([0-9][0-9_]*)\s*\)/m.exec(config);
  if (!sourceMatch) {
    throw new Error("src/config.rs: cannot parse GROK_SEARCH_RESPONSE_MAX_CHARS literal default from response_max_chars");
  }
  const sourceDefault = Number(sourceMatch[1].replaceAll("_", ""));
  const docs = read("docs/CONFIGURATION.md");
  const rows = [...docs.matchAll(/^\|\s*`GROK_SEARCH_RESPONSE_MAX_CHARS`\s*\|\s*`?([0-9][0-9_,]*)`?\s*\|/gm)];
  if (rows.length !== 1) {
    throw new Error(`docs/CONFIGURATION.md must contain exactly one parseable GROK_SEARCH_RESPONSE_MAX_CHARS table row (found ${rows.length})`);
  }
  const documentedDefault = Number(rows[0][1].replaceAll("_", "").replaceAll(",", ""));
  if (documentedDefault !== sourceDefault) {
    fail("response budget", `docs default=${documentedDefault}, expected ${sourceDefault} from src/config.rs`);
  }
});

run("doctor labels", () => {
  const doctor = extractRustFunction(read("src/service.rs"), "doctor", "src/service.rs");
  const responses = /Transport::Responses\s*=>\s*\(\s*"([^"]+)"/m.exec(doctor)?.[1];
  const compatible = /Transport::ChatCompletions\s*=>\s*\(\s*"([^"]+)"/m.exec(doctor)?.[1];
  if (!responses || !compatible) {
    throw new Error("src/service.rs: cannot parse provider labels from doctor() transport match");
  }
  if (responses !== "grok_responses" || compatible !== "openai_compatible") {
    fail(
      "doctor labels",
      `doctor() provider labels changed: Responses=${responses}, ChatCompletions=${compatible}; expected grok_responses/openai_compatible`,
    );
  }

  const lines = read("README.md").split(/\r?\n/);
  const doctorLines = lines
    .map((line, index) => ({ line, index }))
    .filter(({ line }) => /(?:call\s+doctor|grok-search-rs\s+doctor)/i.test(line));
  if (doctorLines.length === 0) throw new Error("README.md has no doctor command or tool-call example");
  const context = doctorLines
    .map(({ index }) => lines.slice(Math.max(0, index - 3), index + 14).join("\n"))
    .join("\n");
  for (const label of [responses, compatible]) {
    if (!context.includes(label)) {
      fail("doctor labels", `README.md doctor example does not contain source provider label ${label}`);
    }
  }
});

run("legacy provider rule", () => {
  if (/`?anthropic`?\s+means\b/i.test(read("CONTRIBUTING.md"))) {
    fail("legacy provider rule", "CONTRIBUTING.md still contains the obsolete `anthropic` means rule");
  }
});

run("repository URL", () => {
  const cargoRepository = tomlString(packageSection(read("Cargo.toml")), "repository");
  const npm = readJson("npm/grok-search-rs/package.json");
  const npmRepository = typeof npm.repository === "string" ? npm.repository : npm.repository?.url;
  let cargoNormalized;
  let npmNormalized;
  try {
    cargoNormalized = normalizedRepository(cargoRepository);
  } catch (error) {
    throw new Error(`Cargo.toml ${error.message}`);
  }
  try {
    npmNormalized = normalizedRepository(npmRepository);
  } catch (error) {
    throw new Error(`npm/grok-search-rs/package.json ${error.message}`);
  }
  if (cargoNormalized !== npmNormalized) {
    fail("repository URL", `Cargo repository ${cargoRepository} does not match npm repository ${npmRepository}`);
  }
});

run("secret scan", () => {
  const docsDir = join(root, "docs");
  let docs;
  try {
    docs = readdirSync(docsDir, { withFileTypes: true })
      .filter((entry) => entry.isFile() && entry.name.endsWith(".md"))
      .map((entry) => `docs/${entry.name}`)
      .sort();
  } catch (error) {
    throw new Error(`cannot enumerate docs/*.md: ${error.message}`);
  }
  if (docs.length === 0) throw new Error("docs/*.md matched no files");
  const files = [
    "README.md",
    "CONTRIBUTING.md",
    ...docs,
    "npm/grok-search-rs/README.md",
    ".env.example",
  ];
  const tokenPattern = /\b(xai|tvly|fc|sk)-[A-Za-z0-9][A-Za-z0-9_-]{19,}\b/g;
  const privateKeyPattern = /-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----/g;
  const placeholderPattern = /\.\.\.|your[-_]?key|placeholder|example|sample|fake|dummy|redacted|replace|changeme/i;

  for (const file of files) {
    const lines = read(file).split(/\r?\n/);
    lines.forEach((line, index) => {
      for (const match of line.matchAll(tokenPattern)) {
        if (!placeholderPattern.test(match[0])) {
          fail("secret scan", `${file}:${index + 1} contains a potential ${match[1]} credential (value redacted)`);
        }
      }
      if (privateKeyPattern.test(line)) {
        fail("secret scan", `${file}:${index + 1} contains a private-key PEM header`);
      }
      privateKeyPattern.lastIndex = 0;
    });
  }
});

if (failures.length > 0) {
  console.error(`Public contract check FAILED (${failures.length} issue${failures.length === 1 ? "" : "s"}):`);
  failures.forEach((failure) => console.error(`- ${failure}`));
  process.exitCode = 1;
} else {
  console.log(`PASS public contract (${passed}/${total} checks)`);
}
