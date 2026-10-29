#!/usr/bin/env bash
# Everything CI checks: tools/lint.sh, then tools/coverage.sh.
set -euo pipefail
cd "$(dirname "$0")/.."

tools/lint.sh
tools/coverage.sh
