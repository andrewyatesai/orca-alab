# Repo-specific configuration for the central publication engine.
STAGING_REMOTE_DEFAULT="git@gh-andrewyatesai:andrewyatesai/orca-alab-staging.git"
CHECK_CMD_DEFAULT="test -s README.md && test -s LICENSE && test -s THIRD-PARTY-NOTICES.md && test -s FEATURE_WALKTHROUGH.md && test -s .gitleaks.toml"
VERSION_DEFAULT="0.1.0"
# Initial public orc landing snapshot. App-source publication is a separate audit.
