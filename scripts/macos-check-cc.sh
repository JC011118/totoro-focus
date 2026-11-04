#!/usr/bin/env bash
# Check-only stand-in for the macOS C/Objective-C compiler, used by
# scripts/check-macos.sh when type-checking the macOS target from Linux.
# `cargo check` never links, so build scripts (e.g. objc2-exception-helper)
# only need *an* object file. It claims to be clang for cc-rs's compiler
# detection and emits an empty host object for each `-o` target.
# Never use this to build real binaries.
out=""; prev=""; pre=0
for a in "$@"; do
  [ "$a" = "-?" ] && exit 1
  [ "$a" = "-E" ] && pre=1
  if [ "$prev" = "-o" ]; then out="$a"; fi
  prev="$a"
done
if [ $pre = 1 ]; then echo '"clang"'; echo '"gcc"'; exit 0; fi
if [ -n "$out" ]; then echo "" | gcc -c -x c - -o "$out"; exit $?; fi
exit 0
