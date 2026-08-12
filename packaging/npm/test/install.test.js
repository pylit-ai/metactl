const assert = require("node:assert/strict");
const test = require("node:test");

const { warnUnverifiedAttestation } = require("../scripts/install.js");

test("npm checksum-only fallback emits an actionable provenance warning", () => {
  const messages = [];
  const archive = "metactl-v0.1.21-x86_64-unknown-linux-gnu.tar.gz";

  warnUnverifiedAttestation(archive, message => messages.push(message));

  assert.equal(messages.length, 1);
  assert.match(messages[0], /SHA-256 verified/);
  assert.match(messages[0], /build provenance was not verified automatically/);
  assert.match(messages[0], new RegExp(archive));
  assert.match(messages[0], /docs\/user\/install-verification\.md/);
});
