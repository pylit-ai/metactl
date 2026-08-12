const crypto = require("node:crypto");
const fs = require("node:fs");
const https = require("node:https");
const os = require("node:os");
const path = require("node:path");
const { spawnSync } = require("node:child_process");

const pkg = require("../package.json");
const targets = {
  "linux-x64": "x86_64-unknown-linux-gnu",
  "darwin-arm64": "aarch64-apple-darwin"
};
const target = targets[`${process.platform}-${process.arch}`];
const requestedVersion = process.env.METACTL_VERSION || pkg.version;
if (!target) {
  throw new Error(`No prebuilt metactl release for ${process.platform}-${process.arch}; use cargo install metactl --version ${requestedVersion} --locked.`);
}

function warnUnverifiedAttestation(archive, warn = console.warn) {
  warn(
    `metactl install warning: SHA-256 verified for ${archive}, but build provenance was not ` +
    "verified automatically. Verify it manually with GitHub CLI: " +
    "https://github.com/pylit-ai/metactl/blob/main/docs/user/install-verification.md"
  );
}

function get(url) {
  return new Promise((resolve, reject) => {
    https.get(url, { headers: { "User-Agent": "metactl-npm-installer" } }, response => {
      if (response.statusCode >= 300 && response.statusCode < 400 && response.headers.location) {
        response.resume();
        const redirect = new URL(response.headers.location, url);
        if (redirect.protocol !== "https:") {
          reject(new Error(`Refusing non-HTTPS redirect for ${url}`));
          return;
        }
        resolve(get(redirect));
        return;
      }
      if (response.statusCode !== 200) {
        response.resume();
        reject(new Error(`Download failed (${response.statusCode}) for ${url}`));
        return;
      }
      const chunks = [];
      response.on("data", chunk => chunks.push(chunk));
      response.on("end", () => resolve(Buffer.concat(chunks)));
    }).on("error", reject);
  });
}

async function install() {
  const tag = `v${requestedVersion}`;
  const archive = `metactl-${tag}-${target}.tar.gz`;
  const base = `https://github.com/pylit-ai/metactl/releases/download/${tag}`;
  const [archiveBytes, checksumBytes] = await Promise.all([get(`${base}/${archive}`), get(`${base}/${archive}.sha256`)]);
  const expected = checksumBytes.toString("utf8").trim().split(/\s+/)[0].toLowerCase();
  const actual = crypto.createHash("sha256").update(archiveBytes).digest("hex");
  if (!/^[a-f0-9]{64}$/.test(expected) || actual !== expected) throw new Error(`SHA-256 verification failed for ${archive}`);
  warnUnverifiedAttestation(archive);

  const temporary = fs.mkdtempSync(path.join(os.tmpdir(), "metactl-npm-"));
  const archivePath = path.join(temporary, archive);
  const vendor = path.join(__dirname, "..", "vendor");
  fs.writeFileSync(archivePath, archiveBytes);
  fs.rmSync(vendor, { recursive: true, force: true });
  fs.mkdirSync(vendor, { recursive: true });
  const extracted = spawnSync("tar", ["-xzf", archivePath, "-C", temporary], { stdio: "inherit" });
  if (extracted.status !== 0) throw new Error(`Could not extract ${archive}`);
  const binary = path.join(temporary, archive.slice(0, -7), "metactl");
  fs.copyFileSync(binary, path.join(vendor, "metactl"));
  fs.chmodSync(path.join(vendor, "metactl"), 0o755);
  fs.rmSync(temporary, { recursive: true, force: true });
}

if (require.main === module) {
  install().catch(error => { console.error(`metactl install failed: ${error.message}`); process.exit(1); });
}

module.exports = { warnUnverifiedAttestation };
