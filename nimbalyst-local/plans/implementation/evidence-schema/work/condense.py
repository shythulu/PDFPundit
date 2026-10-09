"""Pretty-print JSON with scalar arrays and small flat objects on one line, for display."""
import json, sys

def fmt(v, ind=0):
    pad = "  " * ind
    if isinstance(v, dict):
        if not v:
            return "{}"
        flat = all(not isinstance(x, (dict, list)) for x in v.values())
        one = json.dumps(v, ensure_ascii=False, separators=(", ", ": "))
        if flat and len(one) + len(pad) <= 96:
            return one
        items = [f'{pad}  {json.dumps(k)}: {fmt(x, ind + 1)}' for k, x in v.items()]
        return "{\n" + ",\n".join(items) + "\n" + pad + "}"
    if isinstance(v, list):
        if not v:
            return "[]"
        one = json.dumps(v, ensure_ascii=False, separators=(", ", ": "))
        if len(one) + len(pad) <= 96 and all(not isinstance(x, dict) for x in v):
            return one
        items = [f"{pad}  {fmt(x, ind + 1)}" for x in v]
        return "[\n" + ",\n".join(items) + "\n" + pad + "]"
    return json.dumps(v, ensure_ascii=False)

print(fmt(json.load(open(sys.argv[1]))))
