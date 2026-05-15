#!/usr/bin/env node

const { spawn } = require("child_process");
const path = require("path");
const os = require("os");

const PACKAGE_NAME = "grok-search-rs";
const PLATFORMS = {
  "darwin-x64": "@epsiekygrr_zedxx/grok-search-rs-darwin-universal",
  "darwin-arm64": "@epsiekygrr_zedxx/grok-search-rs-darwin-universal",
  "linux-x64": "@epsiekygrr_zedxx/grok-search-rs-linux-x64",
  "linux-arm64": "@epsiekygrr_zedxx/grok-search-rs-linux-arm64",
  "win32-x64": "@epsiekygrr_zedxx/grok-search-rs-win32-x64",
  "win32-arm64": "@epsiekygrr_zedxx/grok-search-rs-win32-arm64",
};

function getBinaryPath() {
  const platformKey = `${process.platform}-${process.arch}`;
  const pkgName = PLATFORMS[platformKey];

  if (!pkgName) {
    console.error(`Unsupported platform: ${process.platform}-${process.arch}`);
    console.error(`Supported platforms: ${Object.keys(PLATFORMS).join(", ")}`);
    process.exit(1);
  }

  try {
    const pkgPath = require.resolve(`${pkgName}/package.json`);
    const binName = process.platform === "win32" ? `${PACKAGE_NAME}.exe` : PACKAGE_NAME;
    return path.join(path.dirname(pkgPath), "bin", binName);
  } catch (_) {
    console.error(`Failed to find platform package: ${pkgName}`);
    console.error("This may happen if npm failed to install the optional dependency.");
    console.error("");
    console.error("Try reinstalling:");
    console.error(`  npm install ${PACKAGE_NAME}`);
    console.error("");
    console.error("Or install the platform package directly:");
    console.error(`  npm install ${pkgName}`);
    process.exit(1);
  }
}

function run() {
  const binaryPath = getBinaryPath();
  const child = spawn(binaryPath, process.argv.slice(2), {
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
    process.exit(code ?? 0);
  });
}

run();
