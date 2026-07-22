# Repo-specific transforms applied to the export tree (sourced by publish.sh
# with $EXPORT and $OUT set, and fail/note available). Each transform must
# verify it applied and fail loudly when stale.

# T1: drop the README hero image. Its asset lives under docs/assets/, which the
# central path deny excludes from every export, so the image block would render
# broken on the public landing page. Remove T1 if the hero asset ever moves to
# an exported location.
python3 - "$EXPORT" <<'PY'
import sys
export = sys.argv[1]
path = f"{export}/README.md"
with open(path) as f:
    s = f.read()

hero = (
    '<p align="center">\n'
    '  <img src="docs/assets/readme-hero.jpg" alt="Orca running coding agents in parallel worktrees" width="960" />\n'
    '</p>\n'
)
assert hero in s, f"transform T1 stale: hero image block not found in {path}"
s = s.replace(hero, "", 1)

assert "docs/assets" not in s, f"transform T1 incomplete: docs/assets reference remains in {path}"
with open(path, "w") as f:
    f.write(s)
PY
note "transform T1 applied: docs/assets hero image dropped from exported README"
