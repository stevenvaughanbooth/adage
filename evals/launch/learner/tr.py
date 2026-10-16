import json
def calls(f):
    out, res = [], {}
    for line in open(f):
        try:
            j = json.loads(line)
        except Exception:
            continue
        c = (j.get("message") or {}).get("content")
        if not isinstance(c, list):
            continue
        for b in c:
            if b.get("type") == "tool_use":
                i = b.get("input", {})
                out.append((b["id"], b["name"], i.get("command") or i.get("file_path") or ""))
            if b.get("type") == "tool_result":
                cc = b.get("content")
                if isinstance(cc, list):
                    cc = " ".join(x.get("text", "") for x in cc if isinstance(x, dict))
                res[b["tool_use_id"]] = (bool(b.get("is_error")), str(cc))
    return [(n, c, res.get(i, (None, ""))) for i, n, c in out]
