#!/usr/bin/env python3
"""Generate the rust-migration markdown docs from the orca-functional-map
workflow result JSON. Deterministic; safe to re-run."""
import json
import sys
from pathlib import Path


def find_payload(obj):
    """Locate the {specs, depMap, phasing} object anywhere in the result."""
    if isinstance(obj, dict):
        if "specs" in obj and "phasing" in obj:
            return obj
        for v in obj.values():
            found = find_payload(v)
            if found:
                return found
    if isinstance(obj, list):
        for v in obj:
            found = find_payload(v)
            if found:
                return found
    return None


def bullets(items):
    if not items:
        return "_(none)_\n"
    return "".join(f"- {str(i).strip()}\n" for i in items)


def main():
    src, outdir = sys.argv[1], Path(sys.argv[2])
    raw = Path(src).read_text(encoding="utf-8")
    start, end = raw.find("{"), raw.rfind("}")
    data = json.loads(raw[start : end + 1])
    payload = find_payload(data)
    if not payload:
        print("could not find specs/phasing payload", file=sys.stderr)
        sys.exit(1)
    specs = payload.get("specs", [])
    depmap = payload.get("depMap", {}) or {}
    phasing = payload.get("phasing", {}) or {}
    outdir.mkdir(parents=True, exist_ok=True)

    # ---- functional-map.md ----
    by_area = {}
    for s in specs:
        by_area.setdefault(s.get("area", "other"), []).append(s)
    fm = ["# Orca Functional Map\n",
          "> Generated from the `orca-functional-map` workflow "
          f"({len(specs)} subsystems mapped). Source of truth for the "
          "TS→Rust port: each subsystem lists what it does, its public/IPC "
          "surface, real external dependencies, persistence, cross-platform "
          "concerns, and a Rust-portability assessment.\n",
          "## Contents\n"]
    area_order = ["backend", "renderer", "cli", "relay", "preload", "shared"]
    areas = [a for a in area_order if a in by_area] + [
        a for a in by_area if a not in area_order]
    for a in areas:
        fm.append(f"- **{a}** — " + ", ".join(
            s["name"] for s in by_area[a]) + "\n")
    fm.append("\n")
    for a in areas:
        fm.append(f"\n## {a}\n")
        for s in sorted(by_area[a], key=lambda x: x["name"]):
            rp = s.get("rustPortability", {}) or {}
            fm.append(f"\n### `{s['name']}`\n")
            fm.append(f"\n{s.get('purpose','').strip()}\n")
            fm.append(
                f"\n**Rust portability:** tier=`{rp.get('tier','?')}` · "
                f"effort=`{rp.get('effort','?')}` · target=`{rp.get('targetCrate','?')}`  \n")
            if rp.get("notes"):
                fm.append(f"_{rp['notes'].strip()}_\n")
            fm.append("\n**Capabilities**\n")
            fm.append(bullets(s.get("capabilities")))
            fm.append("\n**Public API / IPC / RPC**\n")
            fm.append(bullets(s.get("publicApi")))
            fm.append("\n**External dependencies**\n")
            fm.append(bullets(s.get("externalDeps")))
            fm.append("\n**Persistence**\n")
            fm.append(bullets(s.get("persistence")))
            fm.append("\n**Cross-platform concerns**\n")
            fm.append(bullets(s.get("crossPlatform")))
    (outdir / "functional-map.md").write_text("".join(fm), encoding="utf-8")

    # ---- dependency-map.md ----
    deps = depmap.get("dependencies", []) or []
    risk_rank = {"high": 0, "medium": 1, "low": 2}
    deps_sorted = sorted(deps, key=lambda d: (
        risk_rank.get(d.get("risk", "low"), 3), d.get("kind", ""), d.get("name", "")))
    dm = ["# Orca Dependency → Rust Replacement Map\n",
          "> Generated from the `orca-functional-map` workflow. Every external "
          "dependency (npm, native addon, shelled-out binary, Electron/OS/"
          "browser API, network service) and its vendored, stripped Rust "
          "replacement. Sorted by risk (highest first).\n",
          "\n| Dependency | Kind | Role | Rust replacement | Vendor / strip strategy | Risk | Used by |\n",
          "| --- | --- | --- | --- | --- | --- | --- |\n"]
    def cell(x):
        return str(x).replace("|", "\\|").replace("\n", " ").strip()
    for d in deps_sorted:
        used = ", ".join(d.get("usedBy", [])[:6])
        if len(d.get("usedBy", [])) > 6:
            used += ", …"
        dm.append("| {} | {} | {} | {} | {} | {} | {} |\n".format(
            cell(d.get("name", "")), cell(d.get("kind", "")),
            cell(d.get("role", "")), cell(d.get("rustReplacement", "")),
            cell(d.get("vendorStrategy", "")), cell(d.get("risk", "")),
            cell(used)))
    if depmap.get("notes"):
        dm.append("\n## Notes\n\n" + depmap["notes"].strip() + "\n")
    (outdir / "dependency-map.md").write_text("".join(dm), encoding="utf-8")

    # ---- migration-plan.md ----
    mp = ["# Orca → Rust Migration Plan\n",
          "> Generated from the `orca-functional-map` workflow. Domain "
          "groupings (proposed crates), ordered phases, critical path, and "
          "risks. Leaf pure-logic first; UI shells last.\n",
          "\n## Domains → proposed crates\n"]
    for dom in phasing.get("domains", []) or []:
        mp.append(f"\n### {dom.get('domain','')}  →  `{dom.get('targetCrate','')}`\n")
        mp.append(f"\n{dom.get('summary','').strip()}\n")
        mp.append("\nSubsystems: " + ", ".join(
            f"`{x}`" for x in dom.get("subsystems", [])) + "\n")
    mp.append("\n## Phases (ordered)\n")
    for i, ph in enumerate(phasing.get("phases", []) or [], 1):
        mp.append(f"\n### Phase {i}: {ph.get('phase','')}\n")
        mp.append(f"\n**Goal:** {ph.get('goal','').strip()}\n")
        mp.append(f"\n**Rationale:** {ph.get('rationale','').strip()}\n")
        mp.append("\n**Includes:** " + ", ".join(
            f"`{x}`" for x in ph.get("includes", [])) + "\n")
    mp.append("\n## Critical path\n\n")
    mp.append(bullets(phasing.get("criticalPath")))
    mp.append("\n## Risks\n\n")
    mp.append(bullets(phasing.get("risks")))
    (outdir / "migration-plan.md").write_text("".join(mp), encoding="utf-8")

    print(f"wrote functional-map.md ({len(specs)} subsystems), "
          f"dependency-map.md ({len(deps)} deps), "
          f"migration-plan.md ({len(phasing.get('phases', []))} phases)")


if __name__ == "__main__":
    main()
