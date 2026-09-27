#!/bin/sh
# Run one command and report it to Sticky Harness as a harness task.
#
# A worked example of the bridge, not part of the app. It shows the pattern a
# custom harness needs: report start, run the work, then report the outcome -
# including a failure, so a crashed run does not leave a task sitting in the
# Harness Tasks window forever.
#
#   ./run-task.sh my-bot "Run the test suite" "My Bot" -- npm test
#
set -u

if [ "$#" -lt 4 ]; then
  echo "usage: $0 <harness-id> <task-title> <display-name> -- <command...>" >&2
  exit 2
fi

harness=$1
task=$2
name=$3
shift 4   # drop the three arguments and the "--" separator

bridge=$(dirname "$0")/../bin/sticky-harness-bridge.mjs

if [ ! -f "$bridge" ]; then
  echo "bridge not found at $bridge; run this from the Sticky Harness repo" >&2
  exit 2
fi

# Report the start first, so the task appears in the window while the work runs.
if ! node "$bridge" start --harness "$harness" --name "$name" --task "$task"; then
  echo "could not report the task to Sticky Harness; is the app running?" >&2
  exit 2
fi

if "$@"; then
  node "$bridge" done
  exit 0
fi

status=$?
node "$bridge" fail --message "exited with code $status"
exit "$status"
