#!/usr/bin/env python3
"""Private, bounded evaluation ledger for optional skill discovery.

This module deliberately stores metrics and opaque identifiers only. The ledger
and generated reports belong outside source control on a private local volume.
"""

import argparse
import fcntl
import hashlib
import html
import json
import math
import os
from pathlib import Path
import re
import stat
import sys
import time
import uuid


SCHEMA = "metactl.discovery_trial.v1"
MAX_LOG_BYTES = 8 * 1024 * 1024
MAX_EVENT_BYTES = 16 * 1024
MAX_REPORT_BYTES = 2 * 1024 * 1024
HEX64 = re.compile(r"[0-9a-f]{64}\Z")
UUIDHEX = re.compile(r"[0-9a-f]{32}\Z")
MODEL = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,63}(?:/[A-Za-z0-9][A-Za-z0-9._-]{0,63})?\Z")
REASONS = frozenset({"baseline", "disabled", "data_not_authorized", "missing_credential",
                     "budget_exhausted", "unambiguous", "payload_budget",
                     "abstained", "reordered", "unchanged", "deadline",
                     "provider_or_schema_failure", "preferences_unavailable", "session_disabled",
                     "user_disabled", "project_not_enrolled", "project_disabled"})
RUNTIMES = frozenset({"claude-code", "codex-cli", "cursor", "filesystem-agent",
                      "gemini-cli", "openclaw", "opencode",
                      "codex", "omnigent", "pi", "other", "contract"})
ARMS = frozenset({"baseline", "shadow", "advisory"})
TRANSPORTS = frozenset({"direct", "gateway", "none"})
COMMON = {"schema", "kind", "event_id", "session_id", "run_id", "runtime",
          "arm", "transport", "time"}
DISCOVER = {"elapsed_ms", "rank_ms", "result_bytes", "result_count",
            "catalog_digest", "baseline_ids", "effective_ids", "proposed_ids",
            "reason", "provider_attempts", "provider_calls", "usage", "model",
            "native_catalog_suppressed", "cost_usd", "decision_id"}
LOAD = {"elapsed_ms", "result_bytes", "repeat_load", "skill_id"}
OUTCOME = {"success", "task_ms", "input_tokens", "output_tokens", "cost_usd",
           "human_interventions", "verifier_ref"}


def session_key(value):
    if not isinstance(value, str) or not value:
        raise ValueError("session key input must be a nonempty string")
    return hashlib.sha256(value.encode("utf-8")).hexdigest()


def _number(value, label, *, integer=False, nullable=False):
    if value is None and nullable:
        return
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{label} must be nonnegative {'integer' if integer else 'number'}")
    if integer and not isinstance(value, int):
        raise ValueError(f"{label} must be nonnegative integer")
    if not math.isfinite(value) or value < 0:
        raise ValueError(f"{label} must be finite and nonnegative")


def _hex(value, pattern, label):
    if not isinstance(value, str) or not pattern.fullmatch(value):
        raise ValueError(f"{label} must be opaque lowercase hexadecimal identifier")


def validate_event(event):
    if not isinstance(event, dict):
        raise ValueError("event must be an object")
    kind = event.get("kind")
    fields = {"discover": DISCOVER, "load": LOAD, "outcome": OUTCOME}.get(kind)
    if fields is None or event.get("schema") != SCHEMA:
        raise ValueError("unsupported trial schema or kind")
    allowed = COMMON | fields
    extra = event.keys() - allowed
    missing = (COMMON | (fields - {"decision_id"} if kind == "discover" else
                         fields - {"task_ms", "input_tokens", "output_tokens",
                                   "cost_usd", "human_interventions", "verifier_ref"}
                         if kind == "outcome" else fields)) - event.keys()
    if extra or missing:
        raise ValueError(f"event fields invalid: missing={sorted(missing)}, extra={sorted(extra)}")
    _hex(event["event_id"], UUIDHEX, "event_id")
    _hex(event["run_id"], UUIDHEX, "run_id")
    _hex(event["session_id"], HEX64, "session_id")
    if (not isinstance(event["runtime"], str) or event["runtime"] not in RUNTIMES or
            not isinstance(event["arm"], str) or event["arm"] not in ARMS or
            not isinstance(event["transport"], str) or event["transport"] not in TRANSPORTS):
        raise ValueError("invalid runtime, arm or transport")
    _number(event["time"], "time")
    if kind == "discover":
        for key in ("elapsed_ms", "rank_ms"):
            _number(event[key], key)
        for key in ("result_bytes", "result_count", "provider_attempts"):
            _number(event[key], key, integer=True)
        _number(event["provider_calls"], "provider_calls", integer=True, nullable=True)
        if event["provider_calls"] is not None and event["provider_calls"] > event["provider_attempts"]:
            raise ValueError("provider_calls exceeds attempts")
        _hex(event["catalog_digest"], HEX64, "catalog_digest")
        for key in ("baseline_ids", "effective_ids", "proposed_ids"):
            ids = event[key]
            if not isinstance(ids, list) or len(ids) > 100 or len(ids) != len(set(map(str, ids))):
                raise ValueError(f"{key} must be a short unique list")
            for item in ids:
                _hex(item, HEX64, key)
        if not isinstance(event["reason"], str) or event["reason"] not in REASONS:
            raise ValueError("unrecognized reason")
        usage = event["usage"]
        if usage is not None:
            if not isinstance(usage, dict) or set(usage) != {"input_tokens", "output_tokens"}:
                raise ValueError("usage requires input and output tokens")
            for key in usage:
                _number(usage[key], key, integer=True)
        model = event["model"]
        if model is not None and (not isinstance(model, str) or not MODEL.fullmatch(model)):
            raise ValueError("model must be a bounded model identifier")
        if event["native_catalog_suppressed"] is not False or event["cost_usd"] is not None:
            raise ValueError("native suppression and cost are unproven in discover events")
        if "decision_id" in event:
            _hex(event["decision_id"], UUIDHEX, "decision_id")
    elif kind == "load":
        _number(event["elapsed_ms"], "elapsed_ms")
        _number(event["result_bytes"], "result_bytes", integer=True)
        if type(event["repeat_load"]) is not bool:
            raise ValueError("repeat_load must be boolean")
        _hex(event["skill_id"], HEX64, "skill_id")
    else:
        if not isinstance(event["success"], str) or event["success"] not in {"pass", "fail", "unknown"}:
            raise ValueError("success must be pass, fail or unknown")
        if event["success"] != "unknown" and event.get("verifier_ref") is None:
            raise ValueError("pass/fail outcome requires independently supplied verifier_ref")
        for key in ("task_ms", "input_tokens", "output_tokens", "cost_usd", "human_interventions"):
            if key in event:
                _number(event[key], key, integer=key in {"input_tokens", "output_tokens", "human_interventions"}, nullable=True)
        if "verifier_ref" in event and event["verifier_ref"] is not None:
            _hex(event["verifier_ref"], HEX64, "verifier_ref")
    return event


def _open_ledger(path, *, create):
    path = os.fspath(path)
    _check_parent(path)
    flags = os.O_RDWR | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_CLOEXEC", 0)
    try:
        fd = os.open(path, flags)
    except FileNotFoundError:
        if not create:
            raise
        fd = os.open(path, flags | os.O_CREAT | os.O_EXCL, 0o600)
    try:
        info = os.fstat(fd)
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1 or stat.S_IMODE(info.st_mode) != 0o600:
            raise PermissionError("trial ledger must be an owned 0600 regular file with one link")
        return fd
    except BaseException:
        os.close(fd)
        raise


def _check_parent(path):
    """Refuse symlink directory components rather than following them silently."""
    parent = Path(os.path.abspath(path)).parent
    for candidate in (parent, *parent.parents):
        if candidate.is_symlink():
            raise PermissionError("trial path contains symlink directory")


def _read_locked(fd):
    size = os.fstat(fd).st_size
    if size > MAX_LOG_BYTES:
        raise ValueError("trial ledger exceeds size bound")
    os.lseek(fd, 0, os.SEEK_SET)
    data = os.read(fd, size + 1)
    if len(data) != size or (data and not data.endswith(b"\n")):
        raise ValueError("incomplete trial ledger")
    events = []
    seen = {}
    outcomes = set()
    for line in data.splitlines():
        if not line or len(line) > MAX_EVENT_BYTES:
            raise ValueError("invalid trial ledger line")
        try:
            event = json.loads(line.decode("utf-8"), object_pairs_hook=_unique_object)
            validate_event(event)
        except (UnicodeError, json.JSONDecodeError, ValueError, TypeError) as exc:
            raise ValueError("malformed trial ledger event") from exc
        event_id = event["event_id"]
        if event_id in seen:
            if seen[event_id] != event:
                raise ValueError("conflicting duplicate event_id")
            raise ValueError("duplicate event_id")
        if event["kind"] == "outcome":
            key = (event["run_id"], event["session_id"], event["runtime"], event["arm"])
            if key in outcomes:
                raise ValueError("duplicate outcome for session")
            outcomes.add(key)
        seen[event_id] = event
        events.append(event)
    return events


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON field")
        result[key] = value
    return result


def read_events(path):
    fd = _open_ledger(path, create=False)
    try:
        fcntl.flock(fd, fcntl.LOCK_SH | fcntl.LOCK_NB)
        return _read_locked(fd)
    finally:
        os.close(fd)


def record_event(path, event):
    """Append a validated private event; fail closed on persistence errors."""
    validate_event(event)
    line = json.dumps(event, sort_keys=True, separators=(",", ":"), allow_nan=False).encode() + b"\n"
    if len(line) > MAX_EVENT_BYTES:
        raise ValueError("event exceeds size bound")
    fd = _open_ledger(path, create=True)
    try:
        fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
        old = _read_locked(fd)
        for existing in old:
            if existing["event_id"] == event["event_id"]:
                if existing == event:
                    return
                raise ValueError("conflicting duplicate event_id")
            if (event["kind"] == existing["kind"] == "outcome" and
                    (event["run_id"], event["session_id"], event["runtime"], event["arm"]) ==
                    (existing["run_id"], existing["session_id"], existing["runtime"], existing["arm"])):
                raise ValueError("duplicate outcome for session")
        if os.fstat(fd).st_size + len(line) > MAX_LOG_BYTES:
            raise ValueError("trial ledger exceeds size bound")
        os.lseek(fd, 0, os.SEEK_END)
        written = os.write(fd, line)
        if written != len(line):
            raise OSError("short trial ledger write")
        os.fsync(fd)
    finally:
        os.close(fd)


def _percentile(values, percent):
    if not values:
        return None
    values = sorted(values)
    return round(values[math.ceil(percent * len(values)) - 1], 3)


def summarize(events, runtime=None, arm=None):
    if runtime is not None and runtime not in RUNTIMES:
        raise ValueError("invalid runtime filter")
    if arm is not None and arm not in ARMS:
        raise ValueError("invalid arm filter")
    for event in events:
        validate_event(event)
    groups = {}
    selected = [e for e in events if (runtime is None or e["runtime"] == runtime)
                and (arm is None or e["arm"] == arm)]
    for e in selected:
        key = (e["runtime"], e["arm"])
        groups.setdefault(key, []).append(e)
    cohorts = []
    for (rt, cohort_arm), rows in sorted(groups.items()):
        discovers = [e for e in rows if e["kind"] == "discover"]
        loads = [e for e in rows if e["kind"] == "load"]
        outcomes = [e for e in rows if e["kind"] == "outcome"]
        sessions = {e["session_id"] for e in rows}
        outcome_sessions = {e["session_id"] for e in outcomes}
        known_usage = [e["usage"] for e in discovers if e["usage"] is not None]
        known_calls = [e["provider_calls"] for e in discovers if e["provider_calls"] is not None]
        known_cost = [e["cost_usd"] for e in outcomes if e.get("cost_usd") is not None]
        known_task_ms = [e["task_ms"] for e in outcomes if e.get("task_ms") is not None]
        known_task_input = [e["input_tokens"] for e in outcomes if e.get("input_tokens") is not None]
        known_task_output = [e["output_tokens"] for e in outcomes if e.get("output_tokens") is not None]
        known_interventions = [e["human_interventions"] for e in outcomes
                               if e.get("human_interventions") is not None]
        cohorts.append({
            "runtime": rt, "arm": cohort_arm, "sessions": len(sessions),
            "discoveries": len(discovers), "loads": len(loads), "outcomes": len(outcomes),
            "outcome_sessions": len(outcome_sessions),
            "sessions_without_outcome": len(sessions - outcome_sessions),
            "pass": sum(e["success"] == "pass" for e in outcomes),
            "fail": sum(e["success"] == "fail" for e in outcomes),
            "unknown": sum(e["success"] == "unknown" for e in outcomes),
            "fallback": sum(e["reason"] not in {"baseline", "disabled", "unambiguous", "reordered", "unchanged", "abstained"} for e in discovers),
            "skipped": sum(e["reason"] in {"disabled", "unambiguous"} for e in discovers),
            "reordered": sum(e["reason"] == "reordered" and e["proposed_ids"] != e["baseline_ids"] for e in discovers),
            "abstained": sum(e["reason"] == "abstained" for e in discovers),
            "provider_attempts": sum(e["provider_attempts"] for e in discovers),
            "provider_calls_observed": sum(known_calls),
            "provider_calls_known": len(known_calls),
            "uncertain_provider_attempts": sum(e["provider_attempts"] for e in discovers) - sum(known_calls),
            "usage_known": len(known_usage),
            "input_tokens_reported": sum(u["input_tokens"] for u in known_usage) if known_usage else None,
            "output_tokens_reported": sum(u["output_tokens"] for u in known_usage) if known_usage else None,
            "discover_ms_p50": _percentile([e["elapsed_ms"] for e in discovers], .5),
            "discover_ms_p95": _percentile([e["elapsed_ms"] for e in discovers], .95),
            "rank_ms_p50": _percentile([e["rank_ms"] for e in discovers], .5),
            "rank_ms_p95": _percentile([e["rank_ms"] for e in discovers], .95),
            "load_ms_p50": _percentile([e["elapsed_ms"] for e in loads], .5),
            "load_ms_p95": _percentile([e["elapsed_ms"] for e in loads], .95),
            "discovery_result_bytes": sum(e["result_bytes"] for e in discovers),
            "load_result_bytes": sum(e["result_bytes"] for e in loads),
            "repeated_loads": sum(e["repeat_load"] for e in loads),
            "task_ms_known": len(known_task_ms),
            "task_ms_p50": _percentile(known_task_ms, .5),
            "task_ms_p95": _percentile(known_task_ms, .95),
            "task_input_tokens_known": len(known_task_input),
            "task_input_tokens_reported": sum(known_task_input) if known_task_input else None,
            "task_output_tokens_known": len(known_task_output),
            "task_output_tokens_reported": sum(known_task_output) if known_task_output else None,
            "cost_usd_known": len(known_cost),
            "cost_usd_reported": round(sum(known_cost), 6) if known_cost else None,
            "human_interventions_known": len(known_interventions),
            "human_interventions_reported": sum(known_interventions) if known_interventions else None,
        })
    return {"schema": SCHEMA, "event_count": len(selected), "cohorts": cohorts,
            "interpretation": "Descriptive, unmatched cohorts. No causal savings or native catalog suppression established. Result bytes are not prompt tokens. Costs cover independently supplied outcomes only."}


def render_html(report):
    def esc(value):
        return html.escape("unknown" if value is None else str(value), quote=True)

    def coverage(value, known, total):
        return f"{esc(value)} <small>({esc(known)}/{esc(total)} known)</small>"

    def table(title, columns, values):
        heads = "".join(f"<th scope='col'>{esc(label)}</th>" for label in columns)
        body = "".join("<tr>" + "".join(f"<td>{cell}</td>" for cell in row) + "</tr>"
                       for row in values)
        return f"<section><h2>{esc(title)}</h2><table><thead><tr>{heads}</tr></thead><tbody>{body}</tbody></table></section>"

    cohorts = report["cohorts"]
    overview = table("Overview", ["Runtime", "Arm", "Sessions", "Discoveries", "Loads",
                                   "Outcomes", "Missing outcomes", "Repeated loads"],
                     [[esc(r[k]) for k in ("runtime", "arm", "sessions", "discoveries", "loads",
                                             "outcomes", "sessions_without_outcome", "repeated_loads")]
                      for r in cohorts])
    latency = table("Latency and returned bytes", ["Runtime", "Arm", "Discover p50 / p95 ms",
                                                   "Rank p50 / p95 ms", "Load p50 / p95 ms",
                                                   "Discover / load bytes"],
                    [[esc(r["runtime"]), esc(r["arm"]),
                      f"{esc(r['discover_ms_p50'])} / {esc(r['discover_ms_p95'])}",
                      f"{esc(r['rank_ms_p50'])} / {esc(r['rank_ms_p95'])}",
                      f"{esc(r['load_ms_p50'])} / {esc(r['load_ms_p95'])}",
                      esc(f"{r['discovery_result_bytes']} / {r['load_result_bytes']}")]
                     for r in cohorts])
    provider = table("Provider and returned data", ["Runtime", "Arm", "Fallback / reorder / abstain",
                                                    "Attempts", "Validated calls", "Uncertain attempts",
                                                    "Usage coverage", "Input / output tokens"],
                     [[esc(r["runtime"]), esc(r["arm"]),
                       esc(f"{r['fallback']} / {r['reordered']} / {r['abstained']}"),
                       esc(r["provider_attempts"]),
                       coverage(r["provider_calls_observed"] if r["provider_calls_known"] else None,
                                r["provider_calls_known"], r["discoveries"]),
                       esc(r["uncertain_provider_attempts"]),
                       esc(f"{r['usage_known']}/{r['discoveries']} discoveries"),
                       (f"{esc(r['input_tokens_reported'])} / {esc(r['output_tokens_reported'])}"
                        if r["usage_known"] else "unknown")]
                      for r in cohorts])
    outcomes = table("Task outcomes and cost", ["Runtime", "Arm", "Pass / fail / unknown",
                                                "Outcome coverage", "Task p50 / p95 ms",
                                                "Task input / output tokens", "Cost USD", "Interventions"],
                     [[esc(r["runtime"]), esc(r["arm"]),
                       esc(f"{r['pass']} / {r['fail']} / {r['unknown']}"),
                       esc(f"{r['outcome_sessions']}/{r['sessions']} sessions"),
                       (f"{esc(r['task_ms_p50'])} / {esc(r['task_ms_p95'])} "
                        f"<small>({esc(r['task_ms_known'])}/{esc(r['outcomes'])} known)</small>"),
                       (f"in {coverage(r['task_input_tokens_reported'], r['task_input_tokens_known'], r['outcomes'])}<br>"
                        f"out {coverage(r['task_output_tokens_reported'], r['task_output_tokens_known'], r['outcomes'])}"),
                       coverage(r["cost_usd_reported"], r["cost_usd_known"], r["outcomes"]),
                       coverage(r["human_interventions_reported"], r["human_interventions_known"], r["outcomes"]) ]
                      for r in cohorts])
    return ("<!doctype html><html lang='en'><meta charset='utf-8'>"
            "<meta http-equiv='Content-Security-Policy' content=\"default-src 'none'; style-src 'unsafe-inline'\">"
            "<title>Private skill discovery trial</title>"
            "<style>body{font:14px system-ui;max-width:78rem;margin:2rem auto;padding:0 1rem;color:#202830}"
            "section{margin:2rem 0}table{border-collapse:collapse;width:100%;display:block;overflow-x:auto}"
            "th,td{padding:.55rem;text-align:left;border-bottom:1px solid #ccd3dc;white-space:nowrap}"
            "th{background:#edf1f5}tbody tr:nth-child(even){background:#f7f9fb}"
            "small{color:#536274}p{max-width:70rem}</style>"
            "<h1>Private skill discovery trial</h1>"
            f"<p>Events: {esc(report['event_count'])}. {esc(report['interpretation'])}</p>"
            "<p>Known coverage fields give counts with measured values; all other values are unknown. "
            "Compare arms only after independently checking task matching or randomization.</p>"
            f"{overview}{latency}{provider}{outcomes}</html>")


def _write_private(path, content):
    data = content.encode("utf-8")
    if len(data) > MAX_REPORT_BYTES:
        raise ValueError("report exceeds size bound")
    target = Path(path)
    _check_parent(target)
    if target.exists() or target.is_symlink():
        info = target.lstat()
        if not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid() or info.st_nlink != 1 or stat.S_IMODE(info.st_mode) != 0o600:
            raise PermissionError("report target must be an owned 0600 regular file")
    temp = target.with_name(target.name + "." + uuid.uuid4().hex + ".tmp")
    fd = os.open(temp, os.O_WRONLY | os.O_CREAT | os.O_EXCL | getattr(os, "O_NOFOLLOW", 0), 0o600)
    try:
        with os.fdopen(fd, "wb") as out:
            out.write(data)
            out.flush()
            os.fsync(out.fileno())
        os.replace(temp, target)
    finally:
        try:
            temp.unlink()
        except FileNotFoundError:
            pass


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    report = sub.add_parser("report", help="render descriptive private HTML and optional JSON")
    report.add_argument("--log", required=True)
    report.add_argument("--output", required=True)
    report.add_argument("--json-output")
    report.add_argument("--runtime", choices=sorted(RUNTIMES))
    report.add_argument("--arm", choices=sorted(ARMS))
    inspect = sub.add_parser("inspect", help="show recorded calls for one private session")
    inspect.add_argument("--log", required=True)
    inspect.add_argument("--session-id", required=True)
    inspect.add_argument("--run-id")
    outcome = sub.add_parser("outcome", help="attach independently verified task outcome")
    outcome.add_argument("--log", required=True)
    outcome.add_argument("--session-id", required=True)
    outcome.add_argument("--runtime", required=True, choices=sorted(RUNTIMES))
    outcome.add_argument("--arm", required=True, choices=sorted(ARMS))
    outcome.add_argument("--transport", choices=sorted(TRANSPORTS))
    outcome.add_argument("--run-id")
    outcome.add_argument("--success", required=True, choices=["pass", "fail", "unknown"])
    outcome.add_argument("--task-ms", type=float)
    outcome.add_argument("--input-tokens", type=int)
    outcome.add_argument("--output-tokens", type=int)
    outcome.add_argument("--cost-usd", type=float)
    outcome.add_argument("--human-interventions", type=int)
    outcome.add_argument("--verifier-ref", help="opaque SHA-256 identifier only")
    args = parser.parse_args(argv)
    if args.command == "inspect":
        _hex(args.session_id, HEX64, "session_id")
        if args.run_id is not None:
            _hex(args.run_id, UUIDHEX, "run_id")
        rows = [e for e in read_events(args.log) if e["session_id"] == args.session_id
                and (args.run_id is None or e["run_id"] == args.run_id)]
        print(json.dumps({"status": "recorded" if rows else "no_recorded_events",
                          "events": rows, "summary": summarize(rows)}, sort_keys=True))
    elif args.command == "report":
        paths = [os.path.abspath(args.log), os.path.abspath(args.output)]
        if args.json_output:
            paths.append(os.path.abspath(args.json_output))
        if len(paths) != len(set(paths)):
            raise ValueError("ledger and report paths must be distinct")
        result = summarize(read_events(args.log), args.runtime, args.arm)
        _write_private(args.output, render_html(result))
        if args.json_output:
            _write_private(args.json_output, json.dumps(result, indent=2, sort_keys=True) + "\n")
    else:
        matches = {(e["run_id"], e["transport"]) for e in read_events(args.log)
                   if e["session_id"] == args.session_id and e["runtime"] == args.runtime
                   and e["arm"] == args.arm and e["kind"] != "outcome"}
        if args.run_id is not None:
            matches = {pair for pair in matches if pair[0] == args.run_id}
        if args.transport is not None:
            matches = {pair for pair in matches if pair[1] == args.transport}
        if len(matches) != 1:
            raise ValueError("outcome needs exactly one recorded session/run/transport match")
        run_id, transport = next(iter(matches))
        event = {"schema": SCHEMA, "kind": "outcome", "event_id": uuid.uuid4().hex,
                 "session_id": args.session_id, "run_id": run_id, "runtime": args.runtime,
                 "arm": args.arm, "transport": transport, "time": time.time(),
                 "success": args.success}
        for option, name in (("task_ms", "task_ms"), ("input_tokens", "input_tokens"),
                             ("output_tokens", "output_tokens"), ("cost_usd", "cost_usd"),
                             ("human_interventions", "human_interventions"),
                             ("verifier_ref", "verifier_ref")):
            value = getattr(args, option)
            if value is not None:
                event[name] = value
        record_event(args.log, event)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError) as exc:
        print(f"trial ledger error: {exc}", file=sys.stderr)
        sys.exit(1)
