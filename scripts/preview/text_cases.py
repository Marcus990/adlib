"""Hard cases for text fitting and technical diagrams (see scripts/preview_scene.py)."""
def node(i, label, note=None, icon=None): return {"id": f"n{i}", "label": label, "note": note, "icon": icon}
def diagram(layout, title, labels, edges=None, notes=None, icons=None):
    nodes = [node(i + 1, l, (notes or {}).get(l), (icons or {}).get(l)) for i, l in enumerate(labels)]
    ids = {l: f"n{i + 1}" for i, l in enumerate(labels)}
    if edges:
        es = [{"from": ids[a], "to": ids[b], "label": lab} for a, b, lab in edges]
    else:  # what Canvas::auto_link builds: a chain, a closed chain for a cycle, spokes for a hub
        n = [x["id"] for x in nodes]
        if layout == "hub": es = [{"from": n[0], "to": x, "label": None} for x in n[1:]]
        else:
            es = [{"from": n[i - 1], "to": n[i], "label": None} for i in range(1, len(n))]
            if layout == "cycle" and len(n) >= 3: es.append({"from": n[-1], "to": n[0], "label": None})
    return {"layout": layout, "title": title, "nodes": nodes, "edges": es, "auto_edges": not edges}
def tile(id, kind, **kw): return {"id": id, "kind": kind, **kw}
def chart(kind, title, unit, pts): return {"kind": kind, "title": title, "unit": unit, "points": [{"label": l, "value": v} for l, v in pts]}
def grid(*els):
    r = [(0.02, 0.02), (0.51, 0.02), (0.02, 0.51), (0.51, 0.51)]
    for e, (x, y) in zip(els, r): e["rect"] = {"x": x, "y": y, "w": 0.47, "h": 0.47}
    return {"layout": "grid", "elements": list(els)}

ARCH = diagram("flow", "How a request flows through our platform",
    ["Browser", "CDN", "API Gateway", "Authentication and authorization service", "Orders service", "Postgres", "Redis cache", "Kafka event stream"],
    [("Browser", "CDN", None), ("CDN", "API Gateway", "cache miss"), ("API Gateway", "Authentication and authorization service", "verify token"),
     ("API Gateway", "Orders service", "create order"), ("Orders service", "Postgres", "write"), ("Orders service", "Redis cache", "read through"),
     ("Orders service", "Kafka event stream", "publish OrderCreated")])
CLOUDS = chart("bar", "Cloud market share across the three biggest providers worldwide", "%",
    [("Amazon Web Services", 32), ("Microsoft Azure", 23), ("Google Cloud Platform", 11), ("Alibaba Cloud", 4), ("Oracle Cloud Infrastructure", 3), ("IBM Cloud", 2)])
PIE = chart("pie", "Where our infrastructure budget goes each month", "%",
    [("Compute and containers", 41), ("Managed databases", 22), ("Networking and CDN egress", 14), ("Observability", 9), ("Security tooling", 8), ("Other", 3), ("Support", 3)])
CASES = [
    {"name": "arch-full", "scene": {"elements": [tile("e1", "diagram", diagram=ARCH, focus=True)]}},
    {"name": "bar-long-labels-full", "scene": {"elements": [tile("e1", "chart", chart=CLOUDS, focus=True)]}},
    {"name": "pie-long-labels-full", "scene": {"elements": [tile("e1", "chart", chart=PIE, focus=True)]}},
    {"name": "grid-mixed", "scene": grid(
        tile("e1", "diagram", diagram=diagram("flow", "Authentication and authorization flow", ["Client application", "Identity provider", "Token validation service", "Protected resource"])),
        tile("e2", "chart", chart=CLOUDS),
        tile("e3", "diagram", diagram=diagram("hub", "The core platform and everything around it", ["Platform team", "Developer experience", "Reliability engineering", "Security and compliance", "Data infrastructure"])),
        tile("e4", "chart", chart=chart("line", "Requests per second over the launch week", "req/s", [("Monday morning", 1200), ("Tuesday", 4800), ("Wednesday", 15200), ("Thursday", 9100), ("Friday afternoon", 22000)])))},
    {"name": "grid-cycle-timeline-stat", "scene": grid(
        tile("e1", "diagram", diagram=diagram("cycle", "The continuous delivery loop", ["Write code", "Automated tests", "Build container image", "Deploy to staging", "Promote to production"])),
        tile("e2", "diagram", diagram=diagram("timeline", "How the platform evolved over five years", ["Monolith on a single server", "Moved to containers", "Adopted Kubernetes", "Multi region failover", "Serverless data pipeline"], notes={"Monolith on a single server": "2019", "Moved to containers": "2020", "Adopted Kubernetes": "2021", "Multi region failover": "2023", "Serverless data pipeline": "2024"})),
        tile("e3", "chart", chart=chart("stat", "Monthly recurring revenue for the enterprise segment", "$", [("Last quarter", 1240000)])),
        tile("e4", "chart", chart=chart("bar", "Signups", "users", [("Jan", 4000), ("Feb", 5500), ("Mar", 12000)])))},
]


# ---- icons: real files from the card, picked by the same exact-name rules the app uses ----
import json as _json, urllib.parse as _up
_ICONS = "/Volumes/NO NAME/assets/icons/"
try:
    _M = {e["id"]: e for e in _json.load(open(_ICONS + "manifest.json"))}
    _L = _json.load(open(_ICONS + "lookup.json"))
except Exception:
    _M, _L = {}, {}
def _slug(s): return "".join(c for c in s.lower() if c.isalnum())
def pic(kind, name):
    """file:// url of the library entry with this exact name (kind logo | icon), or None."""
    i = _L.get(kind, {}).get(_slug(name))
    return "file://" + _up.quote(_ICONS + _M[i]["file"]) if i else None

ARCH_ICONS = diagram("flow", "How a request flows through our platform",
    ["Browser", "CDN", "API Gateway", "Authentication and authorization service", "Orders service", "Postgres", "Redis cache", "Kafka event stream"],
    [("Browser", "CDN", None), ("CDN", "API Gateway", "cache miss"), ("API Gateway", "Authentication and authorization service", "verify token"),
     ("API Gateway", "Orders service", "create order"), ("Orders service", "Postgres", "write"), ("Orders service", "Redis cache", "read through"),
     ("Orders service", "Kafka event stream", "publish OrderCreated")],
    icons={"Browser": pic("icon", "globe"), "CDN": pic("icon", "cloud"), "API Gateway": pic("icon", "network"), "Authentication and authorization service": pic("icon", "lock"),
           "Orders service": pic("icon", "server"), "Postgres": pic("logo", "Postgres"), "Redis cache": pic("logo", "Redis"), "Kafka event stream": pic("logo", "Kafka")})
CLOUD_LOGOS = chart("bar", "Cloud market share", "%", [("AWS", 32), ("Azure", 23), ("Google Cloud", 11), ("Alibaba Cloud", 4), ("Oracle Cloud", 3), ("IBM Cloud", 2)])
for _p, _n in zip(CLOUD_LOGOS["points"], ["AWS", "Microsoft Azure", "Google Cloud", "Alibaba Cloud", "Oracle", "IBM"]): _p["icon"] = pic("logo", _n)
PIE_LOGOS = chart("pie", "Where the database budget goes", "%", [("PostgreSQL", 38), ("MongoDB", 24), ("Redis", 18), ("MySQL", 12), ("Elasticsearch", 8)])
for _p in PIE_LOGOS["points"]: _p["icon"] = pic("logo", _p["label"])
CASES += [
    {"name": "arch-icons-full", "scene": {"elements": [tile("e1", "diagram", diagram=ARCH_ICONS, focus=True)]}},
    {"name": "charts-logos-grid", "scene": grid(
        tile("e1", "chart", chart=CLOUD_LOGOS), tile("e2", "chart", chart=PIE_LOGOS),
        tile("e3", "diagram", diagram=diagram("flow", "Where the data goes", ["Kafka", "Spark", "Snowflake", "Grafana"], icons={"Kafka": pic("logo", "Kafka"), "Spark": pic("logo", "Apache Spark"), "Snowflake": pic("logo", "Snowflake"), "Grafana": pic("logo", "Grafana")})),
        tile("e4", "diagram", diagram=diagram("hub", "One platform, many teams", ["Platform", "Data", "Security", "Mobile", "Web"], icons={"Platform": pic("icon", "layers"), "Data": pic("icon", "database"), "Security": pic("icon", "shield"), "Mobile": pic("icon", "smartphone"), "Web": pic("icon", "globe")})))},
]
