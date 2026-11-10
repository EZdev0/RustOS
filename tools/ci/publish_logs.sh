#!/usr/bin/env bash
# TEMPORARY diagnostic helper: publish CI logs to the `ci-logs` branch.
# Job logs and artifacts are not reachable from every environment; a branch is.
# Deleted again once the pipeline is green.
set -euo pipefail

work="$(mktemp -d)"
for file in clippy.log boot.log boot-*.log boot-*.status \
            target/qemu_serial.log target/qemu_serial-*.log \
            qemu_output.log clippy-*.log; do
  if [ -f "$file" ]; then
    cp -f "$file" "$work/$(basename "$file")"
  fi
done
if [ -z "$(ls -A "$work" 2>/dev/null)" ]; then
  echo "nothing to publish"
  exit 0
fi

cd "$work"
git init -q
git checkout -q -b ci-logs
git add -A
git -c user.name="ci" -c user.email="ci@localhost" commit -qm "logs: ${GITHUB_JOB:-job} run ${GITHUB_RUN_ID:-0} attempt ${GITHUB_RUN_ATTEMPT:-0}"
git push -q -f "https://x-access-token:${GH_TOKEN}@github.com/${GITHUB_REPOSITORY}.git" ci-logs
echo "published $(ls -A "$work" | wc -l) file(s) to ci-logs"
