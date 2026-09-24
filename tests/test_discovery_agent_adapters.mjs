// Offline Pi bridge contract test. Set PI_PACKAGE_ROOT to an installed Pi package.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const piRoot = process.env.PI_PACKAGE_ROOT;
if (!piRoot || !fs.existsSync(path.join(piRoot, "node_modules/jiti"))) {
  console.log("SKIP: set PI_PACKAGE_ROOT to an installed Pi package");
  process.exit(0);
}
const piRequire = createRequire(path.join(piRoot, "package.json"));
const { createJiti } = piRequire("jiti");
const jiti = createJiti(import.meta.url, { alias: { typebox: piRequire.resolve("typebox") } });
const loaded = jiti(path.join(root, "examples/skill-discovery/pi-extension.ts"));

const mockHost = String.raw`
import json, os, sys, time
for line in sys.stdin:
    request = json.loads(line)
    if request['method'] == 'initialize':
        result = {'protocolVersion':'2025-11-25','capabilities':{'tools':{}},'serverInfo':{'name':'mock','version':'1'}}
    elif request['method'] == 'tools/call':
        args = request['params']['arguments']
        if args.get('query') == 'malformed':
            print('this is not JSON', flush=True)
            continue
        if args.get('query') == 'slow':
            time.sleep(2)
        result = {'isError':False,'content':[{'type':'text','text':json.dumps({'pid':os.getpid(),'name':request['params']['name'],'args':args})}]}
    else:
        result = {}
    print(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':result}), flush=True)
`;
const python = process.env.PYTHON || "python3";
process.env.METACTL_DISCOVERY_COMMAND = python;
process.env.METACTL_DISCOVERY_ARGS_JSON = JSON.stringify(["-u", "-c", mockHost]);

function extension() {
  const tools = new Map();
  const events = new Map();
  (loaded.default || loaded)({
    registerTool: (tool) => tools.set(tool.name, tool),
    on: (event, handler) => events.set(event, handler),
  });
  assert.deepEqual([...tools.keys()], ["discover_skills", "load_skill"]);
  return { tools, close: () => events.get("session_shutdown")() };
}
const first = extension();
const discover = first.tools.get("discover_skills");
const loadSkill = first.tools.get("load_skill");
const one = JSON.parse((await discover.execute("a", { query: "review" })).content[0].text);
const two = JSON.parse((await loadSkill.execute("b", { id: "a".repeat(64), digest: "b".repeat(64) })).content[0].text);
assert.equal(one.pid, two.pid, "calls must share one host and one request budget");
assert.equal(two.name, "load_skill");
assert.equal(two.args.digest, "b".repeat(64));
first.close();

const broken = extension();
await assert.rejects(broken.tools.get("discover_skills").execute("c", { query: "malformed" }), /unavailable for this session/);
await assert.rejects(broken.tools.get("discover_skills").execute("d", { query: "review" }), /unavailable for this session/);
broken.close();
const cancelled = extension();
const controller = new AbortController();
const inflight = cancelled.tools.get("discover_skills").execute("e", { query: "slow" }, controller.signal);
setTimeout(() => controller.abort(), 30);
await assert.rejects(inflight, /unavailable for this session/);
await assert.rejects(cancelled.tools.get("discover_skills").execute("f", { query: "review" }), /unavailable for this session/);
cancelled.close();
console.log("PASS: Pi tools share one MCP child, preserve output, and fail closed after malformed reply or cancellation");
