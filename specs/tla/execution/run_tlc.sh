#!/bin/sh
# Rerun everything. Needs java 11+ and graphviz. JAR defaults to the CI pin.
set -e
JAR=${JAR:-/workspace/builder/tools/tla2tools.jar}
echo "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88  $JAR" | sha256sum -c -
TMP=$(mktemp -d)
# 1. the pinned run (CI command shape): invariants + properties -> tlc.log
CMD="java -XX:+UseParallelGC -Xmx512m -cp $JAR tlc2.TLC -workers 1 -metadir $TMP/main -config OrderLifecycle.cfg OrderLifecycle.tla"
{ echo "\$ $CMD"; $CMD; echo "exit_code=$?"; } > tlc.log 2>&1 || true
# 2. same spec and constants, invariants only, with the state-graph dump
grep -v '^PROPERTY' OrderLifecycle.cfg > OrderLifecycleDump.cfg
CMD="java -XX:+UseParallelGC -Xmx2g -cp $JAR tlc2.TLC -workers 1 -metadir $TMP/dump -dump dot,colorize,actionlabels states.dot -config OrderLifecycleDump.cfg OrderLifecycle.tla"
{ echo "\$ $CMD"; $CMD; echo "exit_code=$?"; } > tlc-dump.log 2>&1 || true
# 3. transitions.json from tlc.log + states.dot
python3 gen_transitions.py tlc.log states.dot transitions.json
# 4. compress the full dump (~1.1 GB raw)
gzip -9 -f states.dot
# 5. reduced-constant dump that graphviz can actually render (states-small.svg)
CMD="java -XX:+UseParallelGC -Xmx512m -cp $JAR tlc2.TLC -workers 1 -metadir $TMP/small -dump dot,colorize,actionlabels states-small.dot -config OrderLifecycleSmall.cfg OrderLifecycle.tla"
{ echo "\$ $CMD"; $CMD; echo "exit_code=$?"; } > tlc-small.log 2>&1 || true
dot -Tsvg states-small.dot -o states-small.svg
# 6. abstracted 9-state lifecycle graph from transitions.json
python3 gen_lifecycle_dot.py transitions.json lifecycle.dot
dot -Tsvg lifecycle.dot -o lifecycle.svg
