#!/usr/bin/env node
const { spawnSync } = require("node:child_process");
const path = require("node:path");

const binary = path.join(__dirname, "..", "vendor", process.platform === "win32" ? "metactl.exe" : "metactl");
const result = spawnSync(binary, process.argv.slice(2), { stdio: "inherit" });
if (result.error) throw result.error;
process.exit(result.status ?? 1);
