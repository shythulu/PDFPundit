"""Build a CASE 1.5.0 JSON-LD view from a PDFPundit custody record.

IDs are UUIDv5 (RFC 9562) in a fixed namespace, named by content, so the file is
deterministic given the record. Usage: case_from_custody.py <record.json> <out.jsonld>
"""
import json
import sys
import uuid

NS = uuid.uuid5(uuid.NAMESPACE_URL, "urn:pdfpundit:case")
rec = json.load(open(sys.argv[1]))
R, run = rec["reproducible"], rec["run"]


def iid(kind, name):
    return f"kb:{kind}-{uuid.uuid5(NS, kind + ':' + name)}"


def hexv(v):
    return {"@type": "xsd:hexBinary", "@value": v}


def hashes(owner, hs):
    out = []
    for algo, method in (("sha256", "SHA256"), ("sha1", "SHA1"), ("md5", "MD5")):
        if hs.get(algo):
            out.append({"@id": iid("hash", f"{owner}:{algo}"), "@type": "uco-types:Hash",
                        "uco-types:hashMethod": method, "uco-types:hashValue": hexv(hs[algo])})
    return out


def file_node(key, name, size, hs, extra_facets=()):
    return {
        "@id": iid("file", key),
        "@type": "uco-observable:File",
        "uco-core:hasFacet": [
            {"@id": iid("filefacet", key), "@type": "uco-observable:FileFacet",
             "uco-observable:fileName": name, "uco-observable:sizeInBytes": size},
            {"@id": iid("contentfacet", key), "@type": "uco-observable:ContentDataFacet",
             "uco-observable:hash": hashes(key, hs)},
            *extra_facets,
        ],
    }


graph = []
tool = {"@id": iid("tool", f"PDFPundit {R['tool']['version']} {R['tool']['build_commit']}"),
        "@type": "uco-tool:Tool", "uco-core:name": "PDFPundit",
        "uco-tool:version": R["tool"]["version"]}
graph.append(tool)

o = run["input_file"]
inp = file_node("sha256:" + o["hashes_before"]["sha256"], o["name"], o["size_bytes"],
                o["hashes_before"])
graph.append(inp)

placed = {p["hashes"]["sha256"]: p for p in run["placed"]}
out_files, img_files = [], []
for p in run["placed"]:
    if p["role"] == "repaired_pdf":
        n = file_node("sha256:" + p["hashes"]["sha256"], p["name"], p["size_bytes"], p["hashes"])
        n["case-investigation:wasDerivedFrom"] = {"@id": inp["@id"]}
        out_files.append(n)
for im in R["images"]["records"]:
    p = placed[im["file_sha256"]]
    d = im["description"]
    facets = []
    if d["width"] is not None:
        facet = {"@id": iid("rasterfacet", im["file_sha256"]),
                 "@type": "uco-observable:RasterPictureFacet",
                 "uco-observable:pictureWidth": d["width"],
                 "uco-observable:pictureHeight": d["height"],
                 "uco-observable:pictureType": im["format"]}
        if d["bits_per_component"] and d["components"]:
            facet["uco-observable:bitsPerPixel"] = d["bits_per_component"] * d["components"]
        facets.append(facet)
    n = file_node("sha256:" + im["file_sha256"], p["name"], p["size_bytes"], p["hashes"], facets)
    n["case-investigation:wasDerivedFrom"] = {"@id": inp["@id"]}
    img_files.append(n)
graph += out_files + img_files

performer = None
if run["case"]["examiner"]:
    performer = {"@id": iid("identity", run["case"]["examiner"]), "@type": "uco-identity:Identity",
                 "uco-core:name": run["case"]["examiner"]}
    graph.append(performer)


def action(name, results):
    a = {"@id": iid("action", f"{name}:{rec['reproducible_sha256']}:{run['log']['seq']}"),
         "@type": "case-investigation:InvestigativeAction",
         "uco-core:name": name,
         "uco-action:startTime": {"@type": "xsd:dateTime", "@value": run["clock"]["started_utc"]},
         "uco-action:endTime": {"@type": "xsd:dateTime", "@value": run["clock"]["ended_utc"]},
         "uco-action:instrument": {"@id": tool["@id"]},
         "uco-action:object": [{"@id": inp["@id"]}],
         "uco-action:result": [{"@id": r["@id"]} for r in results]}
    if performer:
        a["uco-action:performer"] = {"@id": performer["@id"]}
    return a


repair = action("Repair PDF", out_files)
extract = action("Extract images", img_files)
graph += [repair, extract]

if run["case"]["case_reference"]:
    graph.append({"@id": iid("investigation", run["case"]["case_reference"]),
                  "@type": "case-investigation:Investigation",
                  "uco-core:name": run["case"]["case_reference"],
                  "uco-core:object": [{"@id": repair["@id"]}, {"@id": extract["@id"]}]})

doc = {
    "@context": {
        "kb": "urn:pdfpundit:",
        "case-investigation": "https://ontology.caseontology.org/case/investigation/",
        "uco-action": "https://ontology.unifiedcyberontology.org/uco/action/",
        "uco-core": "https://ontology.unifiedcyberontology.org/uco/core/",
        "uco-identity": "https://ontology.unifiedcyberontology.org/uco/identity/",
        "uco-observable": "https://ontology.unifiedcyberontology.org/uco/observable/",
        "uco-tool": "https://ontology.unifiedcyberontology.org/uco/tool/",
        "uco-types": "https://ontology.unifiedcyberontology.org/uco/types/",
        "xsd": "http://www.w3.org/2001/XMLSchema#",
    },
    "@graph": graph,
}
with open(sys.argv[2], "w") as f:
    json.dump(doc, f, indent=2, ensure_ascii=False)
    f.write("\n")
