#!/usr/bin/env bash
# Runs one live probe binary with GPUToolsReplay session hygiene (HANDOFF 4).
# Usage: probes/run.sh <bin-name> [args...]
#
# Enforces: unlock env set, no orphaned replayer before start, one probe at a
# time, and a clean-state check after. Does NOT impose a timeout kill: latency
# is 27s to 20+min, and interrupting orphans the replayer for two hours. Only
# the operator decides to interrupt.
set -euo pipefail

if [ $# -lt 1 ]; then
  echo "usage: $0 <bin-name> [args...]" >&2
  exit 2
fi
BIN="$1"; shift

# -x matches the process NAME exactly. `pgrep -f` matched this script's own
# caller whenever that command line happened to mention the service (a shell
# one-liner that also runs the recovery recipe, say), refusing to run against a
# perfectly clean machine.
if pgrep -x GPUToolsReplayService >/dev/null; then
  echo "REFUSING: a GPUToolsReplayService is already running. An orphaned" >&2
  echo "session locks the replayer. Recover with:" >&2
  echo "  gpudebug --terminate all; pkill -9 -f GPUToolsReplayService" >&2
  exit 1
fi

export MTLREPLAYER_LOCK_PARAM_BUFFER_SIZE_TO_MAX=0

echo "== running probe '$BIN' (elapsed time will be printed; do NOT Ctrl-C unless truly hung) =="
START=$(date +%s)
set +e
cargo run -p replay-probes --bin "$BIN" -- "$@"
CODE=$?
set -e
echo "== probe '$BIN' exited $CODE after $(($(date +%s) - START))s =="

# -x, for the same reason as above.
if pgrep -x GPUToolsReplayService >/dev/null; then
  echo "WARNING: a GPUToolsReplayService is still running after the probe." >&2
  echo "If the next run is refused, recover with:" >&2
  echo "  gpudebug --terminate all; pkill -9 -f GPUToolsReplayService" >&2
fi
exit $CODE
