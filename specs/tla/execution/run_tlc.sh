#!/bin/sh
# Rerun everything (v3.1). Needs java 11+, python3 and graphviz. JAR defaults to the CI pin.
set -e
JAR=${JAR:-/workspace/builder/tools/tla2tools.jar}
echo "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88  $JAR" | sha256sum -c -
ROOT=$(pwd)
TMP=$(mktemp -d .tlc.XXXXXX)
trap 'rm -rf "$TMP"' EXIT
# 1. the pinned run (CI command shape): invariants + properties + REACHED recorder -> tlc.log
#    (-workers 1 is required: RecordReachedCells uses a TLC register)
CMD="java -XX:+UseParallelGC -Xmx512m -cp $JAR tlc2.TLC -workers 1 -metadir $TMP/main -config OrderLifecycle.cfg OrderLifecycle.tla"
if { echo "\$ $CMD"; $CMD; } > "$TMP/tlc.log" 2>&1; then
  echo "exit_code=0" >> "$TMP/tlc.log"
else
  status=$?
  cat "$TMP/tlc.log" >&2
  exit "$status"
fi
# 2. transitions.json + v3 -> v3.1 diff from tlc.log (CELL + REACHED lines); exits nonzero on a failed check
(
cd "$TMP"
python3 "$ROOT/gen_transitions.py" tlc.log transitions.json transitions-v3-to-v3.1.md
# 3. abstracted lifecycle graph from transitions.json
python3 "$ROOT/gen_lifecycle_dot.py" transitions.json lifecycle.dot
dot -Tsvg lifecycle.dot -o lifecycle.svg
)
for artifact in tlc.log transitions.json transitions-v3-to-v3.1.md lifecycle.dot lifecycle.svg; do
  mv "$TMP/$artifact" "$artifact"
done
# 4. mutants/ and findings/ (each variant: TypeOK + its target, same constants)
python3 make_mutants.py "$JAR"
# 5. OPTIONAL, regenerable, not shipped: reduced-constant full state graph (46 MB dot)
if [ "${SMALL_GRAPH:-0}" = 1 ]; then
  CMD="java -XX:+UseParallelGC -Xmx512m -cp $JAR tlc2.TLC -workers 1 -metadir $TMP/small -dump dot,colorize,actionlabels $TMP/states-small.dot -config OrderLifecycleSmall.cfg OrderLifecycle.tla"
  { echo "\$ $CMD"; $CMD; echo "exit_code=0"; } > "$TMP/tlc-small.log" 2>&1
  dot -Tsvg "$TMP/states-small.dot" -o "$TMP/states-small.svg"
  mv "$TMP/tlc-small.log" tlc-small.log
  mv "$TMP/states-small.dot" states-small.dot
  mv "$TMP/states-small.svg" states-small.svg
fi
