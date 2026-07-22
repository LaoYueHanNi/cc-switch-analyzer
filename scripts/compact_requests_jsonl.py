"""One-shot: backup requests.jsonl then compact to minimal fields.

日常维护请用应用内「Hook 备份」：备份后会自动归整（Rust cursor_hook_merge）。
"""
import json
import os
import shutil
import time
from datetime import datetime

USAGE_DIR = os.path.expanduser(r"~/.cursor/local-usage")
SRC = os.path.join(USAGE_DIR, "requests.jsonl")
STAMP = datetime.now().strftime("%Y%m%d-%H%M%S")
BAK_LOCAL = os.path.join(USAGE_DIR, f"requests.jsonl.bak-{STAMP}")
BACKUP_DIR = os.path.expanduser(r"~/.cc-switch-analyzer/hook-backups")
BAK_APP = os.path.join(BACKUP_DIR, f"requests-{STAMP}.jsonl")
TMP = os.path.join(USAGE_DIR, "requests.jsonl.compact.tmp")

ALLOWED = {"ts_utc", "hook_event_name", "model", "model_id", "_parse_error", "_parse_msg"}


def pick_model(row: dict) -> str:
    for key in ("model", "subagent_model", "model_id"):
        v = row.get(key)
        if v is not None and str(v).strip():
            return str(v).strip()
    return ""


def compact_row(row: dict) -> dict:
    out: dict = {}
    ts_utc = row.get("ts_utc")
    if ts_utc and str(ts_utc).strip():
        out["ts_utc"] = str(ts_utc).strip()
    elif row.get("ts"):
        out["ts_utc"] = str(row["ts"]).strip()

    ev = row.get("hook_event_name")
    if ev and str(ev).strip():
        out["hook_event_name"] = str(ev).strip()

    model = pick_model(row)
    if model:
        out["model"] = model
    mid = row.get("model_id")
    if mid and str(mid).strip() and str(mid).strip() != model:
        out["model_id"] = str(mid).strip()
    elif mid and str(mid).strip() and "model" not in out:
        out["model_id"] = str(mid).strip()

    if row.get("_parse_error"):
        out["_parse_error"] = True
        msg = row.get("_parse_msg") or row.get("_empty_stdin")
        if msg is True:
            out["_parse_msg"] = "empty stdin"
        elif msg:
            out["_parse_msg"] = str(msg)
    elif row.get("_empty_stdin"):
        out["_parse_error"] = True
        out["_parse_msg"] = "empty stdin"

    if row.get("_parse_msg") and "_parse_msg" not in out:
        out["_parse_error"] = True
        out["_parse_msg"] = str(row["_parse_msg"])

    if "ts_utc" not in out and not out.get("_parse_error"):
        out["_parse_error"] = True
        out["_parse_msg"] = "missing timestamp after compact"

    return out


def main() -> None:
    if not os.path.isfile(SRC):
        raise SystemExit(f"source missing: {SRC}")

    os.makedirs(BACKUP_DIR, exist_ok=True)
    shutil.copy2(SRC, BAK_LOCAL)
    shutil.copy2(SRC, BAK_APP)

    old_size = os.path.getsize(SRC)
    in_count = 0
    out_count = 0
    parse_fail = 0

    with open(SRC, encoding="utf-8", errors="replace") as fin, open(
        TMP, "w", encoding="utf-8", newline="\n"
    ) as fout:
        for line in fin:
            line = line.strip()
            if not line:
                continue
            in_count += 1
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                parse_fail += 1
                compact = {
                    "ts_utc": datetime.utcnow().strftime("%Y-%m-%dT%H:%M:%S+00:00"),
                    "_parse_error": True,
                    "_parse_msg": "invalid json line",
                }
            else:
                compact = compact_row(row)

            fout.write(json.dumps(compact, ensure_ascii=False, separators=(",", ":")))
            fout.write("\n")
            out_count += 1

    os.replace(TMP, SRC)
    new_size = os.path.getsize(SRC)

    print("backup local:", BAK_LOCAL)
    print("backup app:  ", BAK_APP)
    print("rows in:     ", in_count)
    print("rows out:    ", out_count)
    print("json errors: ", parse_fail)
    print("size before: ", round(old_size / 1024 / 1024, 2), "MB")
    print("size after:  ", round(new_size / 1024 / 1024, 2), "MB")
    print("saved:       ", round((old_size - new_size) / 1024 / 1024, 2), "MB")


if __name__ == "__main__":
    main()
