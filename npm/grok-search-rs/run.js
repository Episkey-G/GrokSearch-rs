#!/usr/bin/env node
const { spawn } = require("child_process");
const path = require("path");
const fs = require("fs");
const https = require("https");
const os = require("os");
const crypto = require("crypto");

const PACKAGE_NAME = "grok-search-rs";
const REPO_OWNER = "Episkey-G";
const REPO_NAME = "GrokSearch-rs";
const REQUEST_TIMEOUT = 60000;
const MAX_REDIRECTS = 10;
const MAX_RETRIES = 3;

function version() {
  return require("./package.json").version;
}

function platformInfo() {
  const platform = process.platform;
  const arch = process.arch;
  if (platform === "darwin" && (arch === "x64" || arch === "arm64")) {
    return {
      packageName: "@epsiekygrr_zedxx/grok-search-rs-darwin-universal",
      assetName: "grok-search-rs_Darwin_universal.tar.gz",
      binaryName: "grok-search-rs",
    };
  }
  if (platform === "linux" && arch === "x64") {
    return {
      packageName: "@epsiekygrr_zedxx/grok-search-rs-linux-x64",
      assetName: "grok-search-rs_Linux_x86_64.tar.gz",
      binaryName: "grok-search-rs",
    };
  }
  if (platform === "linux" && arch === "arm64") {
    return {
      packageName: "@epsiekygrr_zedxx/grok-search-rs-linux-arm64",
      assetName: "grok-search-rs_Linux_aarch64.tar.gz",
      binaryName: "grok-search-rs",
    };
  }
  if (platform === "win32" && arch === "x64") {
    return {
      packageName: "@epsiekygrr_zedxx/grok-search-rs-win32-x64",
      assetName: "grok-search-rs_Windows_x86_64.zip",
      binaryName: "grok-search-rs.exe",
    };
  }
  if (platform === "win32" && arch === "arm64") {
    return {
      packageName: "@epsiekygrr_zedxx/grok-search-rs-win32-arm64",
      assetName: "grok-search-rs_Windows_aarch64.zip",
      binaryName: "grok-search-rs.exe",
    };
  }
  throw new Error(`Unsupported platform: ${platform}/${arch}`);
}

function cacheDir() {
  const home = os.homedir();
  let base;
  if (process.platform === "win32") {
    base = process.env.LOCALAPPDATA || path.join(home, "AppData", "Local");
  } else if (process.platform === "darwin") {
    base = path.join(home, "Library", "Caches");
  } else {
    base = process.env.XDG_CACHE_HOME || path.join(home, ".cache");
  }
  return path.join(base, PACKAGE_NAME, version());
}

function findOptionalBinary(info) {
  try {
    const pkg = require.resolve(`${info.packageName}/package.json`);
    const bin = path.join(path.dirname(pkg), "bin", info.binaryName);
    if (fs.existsSync(bin)) return bin;
  } catch (_) {}
  return null;
}

function requestBuffer(url, options = {}, redirects = 0) {
  return new Promise((resolve, reject) => {
    if (redirects > MAX_REDIRECTS) return reject(new Error("Too many redirects"));
    const req = https.get(url, options, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        res.resume();
        if (!res.headers.location.startsWith("https://")) {
          return reject(new Error(`Insecure redirect: ${res.headers.location}`));
        }
        return requestBuffer(res.headers.location, options, redirects + 1).then(resolve, reject);
      }
      if (res.statusCode !== 200) {
        res.resume();
        return reject(new Error(`HTTP ${res.statusCode}: ${res.statusMessage}`));
      }
      const chunks = [];
      res.on("data", (chunk) => chunks.push(chunk));
      res.on("end", () => resolve(Buffer.concat(chunks)));
    });
    req.on("error", reject);
    req.setTimeout(REQUEST_TIMEOUT, () => {
      req.destroy();
      reject(new Error("Request timeout"));
    });
  });
}

async function withRetry(fn) {
  let last;
  for (let i = 0; i < MAX_RETRIES; i++) {
    try {
      return await fn();
    } catch (err) {
      last = err;
      if (String(err.message).includes("404")) throw err;
      if (i < MAX_RETRIES - 1) {
        await new Promise((resolve) => setTimeout(resolve, 1000 * Math.pow(2, i)));
      }
    }
  }
  throw last;
}

async function releaseForVersion() {
  const tag = `v${version()}`;
  const url = `https://api.github.com/repos/${REPO_OWNER}/${REPO_NAME}/releases/tags/${tag}`;
  const headers = {
    "User-Agent": PACKAGE_NAME,
    Accept: "application/vnd.github.v3+json",
    ...(process.env.GITHUB_TOKEN ? { Authorization: `token ${process.env.GITHUB_TOKEN}` } : {}),
  };
  const data = await withRetry(() => requestBuffer(url, { headers }));
  return JSON.parse(data.toString());
}

function downloadFile(url, dest, options = {}, redirects = 0) {
  return new Promise((resolve, reject) => {
    if (redirects > MAX_REDIRECTS) return reject(new Error("Too many redirects"));
    const file = fs.createWriteStream(dest);
    const req = https.get(url, options, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        res.resume();
        file.close(() => {
          try { fs.unlinkSync(dest); } catch (_) {}
          if (!res.headers.location.startsWith("https://")) {
            return reject(new Error(`Insecure redirect: ${res.headers.location}`));
          }
          downloadFile(res.headers.location, dest, options, redirects + 1).then(resolve, reject);
        });
        return;
      }
      if (res.statusCode !== 200) {
        res.resume();
        file.close(() => {
          try { fs.unlinkSync(dest); } catch (_) {}
          reject(new Error(`HTTP ${res.statusCode}: ${res.statusMessage}`));
        });
        return;
      }
      res.pipe(file);
      file.on("finish", () => file.close(resolve));
    });
    req.on("error", (err) => file.close(() => reject(err)));
    req.setTimeout(REQUEST_TIMEOUT, () => {
      req.destroy();
      file.close(() => reject(new Error("Download timeout")));
    });
  });
}

function extractArchive(archive, dir, info) {
  return new Promise((resolve, reject) => {
    const child = info.assetName.endsWith(".zip")
      ? spawn("powershell", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", `Expand-Archive -LiteralPath '${archive.replace(/'/g, "''")}' -DestinationPath '${dir.replace(/'/g, "''")}' -Force`], { stdio: ["ignore", process.stderr, process.stderr] })
      : spawn("tar", ["-xzf", archive, "-C", dir], { stdio: ["ignore", process.stderr, process.stderr] });
    child.on("error", reject);
    child.on("close", (code) => code === 0 ? resolve() : reject(new Error(`extract exited with code ${code}`)));
  });
}

function acquireLock(lock) {
  try {
    fs.writeFileSync(lock, String(process.pid), { flag: "wx" });
    return true;
  } catch (err) {
    if (err.code !== "EEXIST") throw err;
    return false;
  }
}

async function downloadBinary(info) {
  const dir = cacheDir();
  const bin = path.join(dir, info.binaryName);
  const lock = path.join(dir, ".lock");
  fs.mkdirSync(dir, { recursive: true });
  if (fs.existsSync(bin)) return bin;

  if (!acquireLock(lock)) {
    console.error("Another grok-search-rs process is installing, waiting...");
    for (let i = 0; i < 60; i++) {
      if (fs.existsSync(bin)) return bin;
      await new Promise((resolve) => setTimeout(resolve, 1000));
    }
    throw new Error("Timeout waiting for install lock");
  }

  const tempId = crypto.randomBytes(8).toString("hex");
  const archive = path.join(dir, `${tempId}-${info.assetName}`);
  const extractDir = path.join(dir, `${tempId}-extract`);
  try {
    if (fs.existsSync(bin)) return bin;
    console.error(`Downloading ${PACKAGE_NAME} v${version()}...`);
    const release = await releaseForVersion();
    const asset = release.assets.find((item) => item.name === info.assetName);
    if (!asset) {
      throw new Error(`No matching release asset: ${info.assetName}`);
    }
    const headers = {
      "User-Agent": PACKAGE_NAME,
      Accept: "application/octet-stream",
      ...(process.env.GITHUB_TOKEN ? { Authorization: `token ${process.env.GITHUB_TOKEN}` } : {}),
    };
    await withRetry(() => downloadFile(asset.browser_download_url, archive, { headers }));
    fs.mkdirSync(extractDir, { recursive: true });
    await extractArchive(archive, extractDir, info);
    const extracted = path.join(extractDir, info.binaryName);
    if (!fs.existsSync(extracted)) throw new Error(`Binary not found in archive: ${info.binaryName}`);
    fs.renameSync(extracted, bin);
    if (process.platform !== "win32") fs.chmodSync(bin, 0o755);
    console.error(`Installed ${PACKAGE_NAME} to ${bin}`);
    return bin;
  } finally {
    try { fs.unlinkSync(lock); } catch (_) {}
    try { fs.unlinkSync(archive); } catch (_) {}
    try { fs.rmSync(extractDir, { recursive: true, force: true }); } catch (_) {}
  }
}

async function main() {
  const info = platformInfo();
  const optional = findOptionalBinary(info);
  const binary = optional || await downloadBinary(info);
  const child = spawn(binary, process.argv.slice(2), {
    stdio: "inherit",
    env: process.env,
  });
  for (const signal of ["SIGINT", "SIGTERM", "SIGHUP"]) {
    process.on(signal, () => {
      if (!child.killed) child.kill(signal);
    });
  }
  child.on("error", (err) => {
    console.error(`Failed to start ${PACKAGE_NAME}: ${err.message}`);
    process.exit(1);
  });
  child.on("exit", (code, signal) => {
    if (signal) process.exit(128 + (os.constants.signals[signal] || 0));
    process.exit(code || 0);
  });
}

main().catch((err) => {
  console.error(err.message || err);
  process.exit(1);
});
