#!/usr/bin/env python3
"""Machine-local, explicitly enrolled discovery defaults. No provider access."""
import argparse
import contextlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tempfile


def config_path():
    base = Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config")))
    if not base.is_absolute():
        raise ValueError("XDG_CONFIG_HOME must be absolute")
    return base / "metactl" / "discovery.json"


def validate(doc):
    if not isinstance(doc, dict) or doc.get("schema") != 1:
        raise ValueError("Unsupported discovery preferences")
    if doc.get("mode") not in ("enabled", "disabled"):
        raise ValueError("Invalid discovery mode")
    if type(doc.get("allow_provider_data")) is not bool:
        raise ValueError("Invalid data permission")
    command = doc.get("gateway_command")
    if not isinstance(command, str) or (command and not Path(command).is_absolute()):
        raise ValueError("Gateway command must be absolute")
    calls, deadline = doc.get("max_provider_calls"), doc.get("provider_deadline")
    if type(calls) is not int or not 1 <= calls <= 10:
        raise ValueError("Provider call ceiling must be 1-10")
    if type(deadline) not in (int, float) or not math.isfinite(deadline) or not 0 < deadline <= 5:
        raise ValueError("Provider deadline must be positive and at most 5 seconds")
    if not isinstance(doc.get("projects"), dict):
        raise ValueError("Invalid enrolled projects")
    for root, record in doc["projects"].items():
        if not Path(root).is_absolute() or not isinstance(record, dict):
            raise ValueError("Invalid enrolled project")
        if record.get("mode") not in ("inherit", "disabled"):
            raise ValueError("Invalid project mode")
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,79}", record.get("gateway_project", "")):
            raise ValueError("Invalid gateway project ID")
        if record.get("data_class") not in ("public-nonsensitive", "private-owned"):
            raise ValueError("Invalid project data class")
    return doc


def empty():
    return {"schema": 1, "mode": "disabled", "allow_provider_data": False,
            "gateway_command": "", "max_provider_calls": 4,
            "provider_deadline": 5.0, "projects": {}}


def read_document(path):
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0)
    try:
        fd = os.open(path, flags)
    except FileNotFoundError:
        return empty()
    with os.fdopen(fd) as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > 1048576:
            raise ValueError("Invalid preferences file")
        if os.name == "posix" and (metadata.st_uid != os.getuid() or metadata.st_mode & 0o077):
            raise ValueError("Preferences must be owned by this user with mode 0600")
        return validate(json.load(stream))


def resolve(project):
    """Re-read for each call; errors and missing enrollment always disable routing."""
    result = {"enabled": False, "reason": "preferences_unavailable", "source": "user",
              "reload_required": False, "provider_calls_this_check": 0}
    try:
        path = config_path()
        result["config_path"] = str(path)
        doc = read_document(path)
        record = doc["projects"].get(str(Path(project).resolve()))
        result.update(mode=doc["mode"], project_mode=record["mode"] if record else "not_enrolled")
        if os.environ.get("METACTL_JEV_DISABLE") == "1":
            result["reason"] = "session_disabled"
        elif doc["mode"] != "enabled":
            result["reason"] = "user_disabled"
        elif not record:
            result["reason"] = "project_not_enrolled"
        elif record["mode"] == "disabled":
            result["reason"] = "project_disabled"
        elif not doc["allow_provider_data"]:
            result["reason"] = "data_not_authorized"
        else:
            result.update(enabled=True, reason="enabled", gateway_command=doc["gateway_command"],
                          gateway_project=record["gateway_project"], data_class=record["data_class"],
                          max_provider_calls=doc["max_provider_calls"],
                          provider_deadline=doc["provider_deadline"])
        command = doc["gateway_command"]
        result["gateway_state"] = ("present" if os.path.isfile(command) and os.access(command, os.X_OK)
                                   else "not_executable" if os.path.exists(command) else "missing")
    except (OSError, ValueError, TypeError):
        pass
    return result


@contextlib.contextmanager
def locked(path):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if path.parent.is_symlink() or path.is_symlink():
        raise ValueError("Symlink preferences are not supported")
    lock = path.with_suffix(".lock")
    fd = os.open(lock, os.O_CREAT | os.O_RDWR | getattr(os, "O_NOFOLLOW", 0), 0o600)
    with os.fdopen(fd, "w") as stream:
        if os.name == "posix":
            import fcntl
            fcntl.flock(stream, fcntl.LOCK_EX)
        yield


def save(path, doc):
    validate(doc)
    fd, temporary = tempfile.mkstemp(prefix=".discovery-", dir=path.parent)
    try:
        with os.fdopen(fd, "w") as stream:
            json.dump(doc, stream, indent=2)
            stream.write("\n")
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def main():
    parser = argparse.ArgumentParser(description=(
        "Save Jev defaults once. Enrolled projects send task text and candidate skill metadata "
        "through the gateway to TypeSafe. Provider processing/retention applies. "
        "Exclude secrets and restricted third-party projects. No provider calls here."))
    parser.add_argument("--project", required=True)
    parser.add_argument("--mode", choices=("enabled", "disabled"))
    parser.add_argument("--allow-provider-data", action="store_true")
    parser.add_argument("--revoke-provider-data", action="store_true")
    parser.add_argument("--gateway-command")
    parser.add_argument("--max-provider-calls", type=int)
    parser.add_argument("--provider-deadline", type=float)
    parser.add_argument("--enroll", action="store_true")
    parser.add_argument("--replace-enrollment", action="store_true")
    parser.add_argument("--gateway-project")
    parser.add_argument("--data-class", choices=("public-nonsensitive", "private-owned"))
    parser.add_argument("--project-mode", choices=("inherit", "disabled"))
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()
    try:
        path = config_path()
        mutate = any((args.mode, args.enroll, args.project_mode, args.gateway_command,
                      args.allow_provider_data, args.revoke_provider_data,
                      args.max_provider_calls is not None, args.provider_deadline is not None))
        if mutate:
            with locked(path):
                doc = read_document(path)
                if args.mode == "enabled" and not (args.allow_provider_data or doc["allow_provider_data"]):
                    raise ValueError("Enabling sends task text and skill metadata to TypeSafe; use --allow-provider-data once")
                if args.mode:
                    doc["mode"] = args.mode
                if args.allow_provider_data:
                    doc["allow_provider_data"] = True
                if args.revoke_provider_data:
                    doc["allow_provider_data"] = False
                if args.gateway_command:
                    command = shutil.which(args.gateway_command)
                    if not command:
                        raise ValueError("Gateway executable not found")
                    doc["gateway_command"] = os.path.abspath(command)
                for field in ("max_provider_calls", "provider_deadline"):
                    if getattr(args, field) is not None:
                        doc[field] = getattr(args, field)
                root = str(Path(args.project).resolve(strict=True))
                if args.enroll:
                    if not args.gateway_project or not args.data_class:
                        raise ValueError("Enrollment requires --gateway-project and --data-class")
                    previous = doc["projects"].get(root, {})
                    if previous and not args.replace_enrollment and any(
                            previous.get(key) != value for key, value in
                            (("gateway_project", args.gateway_project), ("data_class", args.data_class))):
                        raise ValueError("Enrollment identity or data class differs; review then use --replace-enrollment")
                    try:
                        check = subprocess.run([doc["gateway_command"], "check-project", "--project", args.gateway_project],
                                               cwd=root, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                               text=True, timeout=5,
                                               env={k: v for k, v in os.environ.items() if k != "TYPESAFE_API_KEY"})
                        match = json.loads(check.stdout)
                        if check.returncode or match.get("project_match") is not True or match.get("project_id") != args.gateway_project:
                            raise ValueError("mismatch")
                    except (OSError, ValueError, subprocess.TimeoutExpired):
                        raise ValueError("Gateway client did not verify this project path and ID; update the client or correct enrollment") from None
                    doc["projects"][root] = {"gateway_project": args.gateway_project,
                                             "data_class": args.data_class,
                                             "mode": previous.get("mode", "inherit")}
                if args.project_mode:
                    if root not in doc["projects"]:
                        raise ValueError("Enroll this project before setting its override")
                    doc["projects"][root]["mode"] = args.project_mode
                if doc["mode"] == "enabled" and not doc["gateway_command"]:
                    raise ValueError("Enabling requires --gateway-command")
                save(path, doc)
        result = resolve(args.project)
        result["saved"] = mutate
        print(json.dumps(result) if args.json else
              "Jev: {reason}\nPreferences: {config_path}\nChanges apply to the next discovery request; "
              "connect each agent with --use-preferences. Provider calls: 0".format(**result))
    except (OSError, ValueError, TypeError) as error:
        parser.exit(2, "Discovery preferences unavailable: " + str(error) + "\n")


if __name__ == "__main__":
    main()
