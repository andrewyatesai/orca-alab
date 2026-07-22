# Repo-specific configuration for the central publication engine.
STAGING_REMOTE_DEFAULT="git@gh-andrewyatesai:andrewyatesai/orca-alab-staging.git"
CHECK_CMD_DEFAULT="test -s README.md && test -s LICENSE && test -s NOTICE && test -s THIRD-PARTY-NOTICES.md && test -s .gitleaks.toml && test -s resources/readme-hero.jpg"
VERSION_DEFAULT="0.2.0"
# Public landing snapshot (README swapped at export by transform T1).
# App-source publication is a separate audit; see publish/DECISIONS.md.
