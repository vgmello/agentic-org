#!/usr/bin/env python3
"""Screen catalog: add or replace one screen's screenshots and record it in screens.json.

Usage: catalog.py <id> --area A --title T --route R --commit SHA --state implemented|validated|reference
                  [--desktop PNG] [--phone PNG] [--drop desktop|phone] [--note TEXT] [--kind app|reference]

Images are stored as img/<id>-desktop.jpg and img/<id>-phone.jpg: the same name on every update,
so a changed screen replaces its old image (git history keeps the old one). A new entry needs at
least one image. --commit is the commit the screenshots show; it changes only with an image.
Nothing is written until every check and conversion has passed, then images and screens.json
are moved into place together.
"""
import argparse, copy, datetime, hashlib, json, os, re, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
DB = os.path.join(HERE, "screens.json")
VIEWS = ("desktop", "phone")
ID_RE = re.compile(r"[a-z0-9][a-z0-9-]*")
MAX_EDGE = 1600


def dims(src):
    r = subprocess.run(["sips", "-g", "pixelWidth", "-g", "pixelHeight", os.path.abspath(src)], capture_output=True, text=True)
    vals = re.findall(r"pixel(?:Width|Height):\s*(\d+)", r.stdout)
    if r.returncode != 0 or len(vals) != 2:
        sys.exit(f"not an image: {src}")
    return int(vals[0]), int(vals[1])


def to_jpeg(src, dst):
    cmd = ["sips", "-s", "format", "jpeg", "-s", "formatOptions", "78"]
    if max(dims(src)) > MAX_EDGE:  # shrink only, never upscale
        cmd += ["-Z", str(MAX_EDGE)]
    r = subprocess.run(cmd + [os.path.abspath(src), "--out", dst], capture_output=True, text=True)
    if r.returncode != 0 or not os.path.exists(dst) or os.path.getsize(dst) == 0:
        sys.exit(f"could not convert {src}: {(r.stderr or r.stdout).strip() or 'no output written'}")
    with open(dst, "rb") as f:
        return hashlib.sha1(f.read()).hexdigest()[:10]


def main():
    p = argparse.ArgumentParser(description="Add or update one screen in the catalog.")
    p.add_argument("id", help="lowercase letters, digits and hyphens, e.g. chat-thread")
    for k in ("area", "title", "route", "commit"):
        p.add_argument("--" + k, required=True)
    p.add_argument("--state", required=True, choices=["implemented", "validated", "reference"])
    p.add_argument("--kind", default="app", choices=["app", "reference"])
    p.add_argument("--desktop"); p.add_argument("--phone")
    p.add_argument("--drop", choices=VIEWS, action="append", default=[], help="remove a view")
    p.add_argument("--note", default="")
    a = p.parse_args()

    # 1. Validate everything before touching disk.
    if not ID_RE.fullmatch(a.id):
        sys.exit("id must be lowercase letters, digits and hyphens")
    given = {v: s for v, s in (("desktop", a.desktop), ("phone", a.phone)) if s}
    if set(given) & set(a.drop):
        sys.exit("a view can't be both supplied and dropped")
    for src in given.values():  # absolute paths below, so a name starting with '-' is never an option
        if not os.path.isfile(src):
            sys.exit(f"no such file: {src}")
    db = {"screens": []}
    if os.path.exists(DB):
        try:
            with open(DB, encoding="utf-8") as f:
                db = json.load(f)
        except (ValueError, OSError) as e:  # ValueError covers bad JSON and bad UTF-8
            sys.exit(f"can't read {DB}: {e}")
        if not isinstance(db, dict) or not isinstance(db.get("screens", []), list):
            sys.exit(f"{DB} is malformed: expected an object with a 'screens' list")
    screens = [s for s in db.get("screens", []) if isinstance(s, dict) and s.get("id")]
    old = next((s for s in screens if s["id"] == a.id), None)
    is_new = old is None
    entry = copy.deepcopy(old) if old else {"id": a.id}
    if not isinstance(entry.get("history"), list):
        entry["history"] = []
    repaired = [v for v in VIEWS if v in entry and not (isinstance(entry[v], dict) and entry[v].get("src"))]
    for v in repaired:  # a broken view record is removed, and the run writes so it doesn't linger
        del entry[v]
    for v in a.drop:
        entry.pop(v, None)
    final_views = set(given) | {v for v in VIEWS if v in entry}
    if not final_views:
        sys.exit("a new screen needs --desktop or --phone" if is_new else "a screen must keep at least one view")

    # Staging sits next to the catalog so os.replace stays on one filesystem; .gitignore
    # keeps a folder left behind by a killed run out of git.
    with tempfile.TemporaryDirectory(dir=HERE, prefix=".staging-") as tmpdir:
        # 2. Convert into a staging folder.
        staged, image_changed = {}, bool(old and any(v in old for v in a.drop))
        for view, src in given.items():
            tmp = os.path.join(tmpdir, f"{view}.jpg")
            h = to_jpeg(src, tmp)
            if (entry.get(view) or {}).get("hash") != h:  # entry holds only well-formed views
                image_changed = True
            staged[view] = (tmp, h)

        # 3. Build the final entry in memory.
        meta = dict(kind=a.kind, area=a.area, title=a.title, route=a.route, state=a.state, note=a.note)
        meta_changed = any(entry.get(k) != v for k, v in meta.items())
        if not (is_new or image_changed or meta_changed or repaired):
            print("unchanged " + a.id)
            return
        now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%MZ")
        if image_changed and entry.get("commit") and entry["commit"] != a.commit:
            entry["history"].insert(0, {"commit": entry["commit"], "updated": entry.get("updated")})
        entry.update(meta)
        for view, (_, h) in staged.items():
            entry[view] = {"src": f"img/{a.id}-{view}.jpg", "hash": h}
        if is_new or image_changed:
            entry["commit"], entry["updated"] = a.commit, now
        db["screens"] = [entry if s is old else s for s in screens] + ([entry] if is_new else [])
        db["generated"] = now
        fd, tmpjson = tempfile.mkstemp(dir=tmpdir, suffix=".json")
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(db, f, indent=1, ensure_ascii=False)
            f.write("\n")
        os.chmod(tmpjson, 0o644)

        # 4. Commit: images, then removals, then the JSON last.
        os.makedirs(os.path.join(HERE, "img"), exist_ok=True)
        for view, (tmp, _) in staged.items():
            os.chmod(tmp, 0o644)
            os.replace(tmp, os.path.join(HERE, f"img/{a.id}-{view}.jpg"))
        for view in set(a.drop) | (set(repaired) - set(staged)):
            try:
                os.remove(os.path.join(HERE, f"img/{a.id}-{view}.jpg"))
            except FileNotFoundError:
                pass
        os.replace(tmpjson, DB)
    for v in repaired:
        if v not in staged:
            print(f"removed broken {v} view record from {a.id}", file=sys.stderr)
    print(("added " if is_new else "updated " if image_changed else "metadata updated ") + a.id)


if __name__ == "__main__":
    main()
