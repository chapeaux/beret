#!/usr/bin/env node
"use strict";

const { existsSync, mkdirSync, createWriteStream, chmodSync, unlinkSync } = require("fs");
const { join } = require("path");
const { get } = require("https");
const { createGunzip } = require("zlib");

const VERSION = require("./package.json").version;
const REPO = "chapeaux/beret";

const PLATFORM_MAP = {
  "darwin-x64": "x86_64-apple-darwin",
  "darwin-arm64": "aarch64-apple-darwin",
  "linux-x64": "x86_64-unknown-linux-gnu",
  "linux-arm64": "aarch64-unknown-linux-gnu",
  "win32-x64": "x86_64-pc-windows-msvc",
  "win32-arm64": "aarch64-pc-windows-msvc",
};

const key = `${process.platform}-${process.arch}`;
const target = PLATFORM_MAP[key];
if (!target) {
  console.error(`Unsupported platform: ${key}`);
  process.exit(1);
}

const ext = process.platform === "win32" ? ".exe" : "";
const binName = `beret${ext}`;
const binDir = join(__dirname, "bin");
const binPath = join(binDir, binName);

if (existsSync(binPath)) {
  process.exit(0);
}

const archiveExt = process.platform === "win32" ? ".zip" : ".tar.gz";
const url = `https://github.com/${REPO}/releases/download/v${VERSION}/beret-v${VERSION}-${target}${archiveExt}`;

function download(url, dest) {
  return new Promise((resolve, reject) => {
    get(url, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        return download(res.headers.location, dest).then(resolve, reject);
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`Download failed: HTTP ${res.statusCode} for ${url}`));
      }

      mkdirSync(binDir, { recursive: true });
      const file = createWriteStream(dest);
      res.pipe(file);
      file.on("finish", () => {
        file.close();
        resolve();
      });
      file.on("error", reject);
    }).on("error", reject);
  });
}

async function main() {
  const tmpPath = join(binDir, `beret-download${archiveExt}`);

  console.log(`Downloading beret v${VERSION} for ${target}...`);
  await download(url, tmpPath);

  if (archiveExt === ".tar.gz") {
    const { execSync } = require("child_process");
    execSync(`tar -xzf "${tmpPath}" -C "${binDir}" --strip-components=1`, { stdio: "inherit" });
  } else {
    const { execSync } = require("child_process");
    execSync(`powershell -Command "Expand-Archive -Path '${tmpPath}' -DestinationPath '${binDir}' -Force"`, { stdio: "inherit" });
  }

  try { unlinkSync(tmpPath); } catch {}

  if (process.platform !== "win32") {
    chmodSync(binPath, 0o755);
  }

  console.log(`Installed beret to ${binPath}`);
}

main().catch((err) => {
  console.error(`Failed to install beret: ${err.message}`);
  console.error("You can install manually: cargo install chapeaux-beret");
  process.exit(1);
});
