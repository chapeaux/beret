#!/usr/bin/env node
"use strict";

const { join } = require("path");
const { spawnSync } = require("child_process");

const ext = process.platform === "win32" ? ".exe" : "";
const binPath = join(__dirname, "bin", `beret${ext}`);

const result = spawnSync(binPath, process.argv.slice(2), {
  stdio: "inherit",
  env: process.env,
});

if (result.error) {
  if (result.error.code === "ENOENT") {
    console.error("beret binary not found. Run: npm rebuild @chapeaux/beret");
    console.error("Or install directly: cargo install chapeaux-beret");
  } else {
    console.error(`Failed to run beret: ${result.error.message}`);
  }
  process.exit(1);
}

process.exit(result.status ?? 1);
