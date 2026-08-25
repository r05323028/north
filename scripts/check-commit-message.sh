#!/usr/bin/env bash
# Conventional Commit validator for North (commit-msg hook + CI PR-title check).
#
# Accepted format:
#   type[(scope)][!]: description
#
# Types: feat fix docs test refactor perf build ci chore revert
# Scope optional. Squash-merge repositories enforce this on the canonical PR
# title instead of every branch commit (docs/development/git-workflow.md).
set -euo pipefail

TYPES='feat|fix|docs|test|refactor|perf|build|ci|chore|revert'
REGEX="^(${TYPES})(\\([^)]+\\))?(!)?: .+$"

if [[ "${1:-}" == "--self-test" ]]; then
  fail=0
  check() { # desc expected input
    local desc="$1" expected="$2" input="$3"
    if echo "$input" | grep -Eq "$REGEX"; then got=ok; else got=reject; fi
    if [[ "$got" == "$expected" ]]; then
      printf 'ok      %-38s (%s)\n' "$desc" "$got"
    else
      printf 'FAIL    %-38s expected=%s got=%s\n' "$desc" "$expected" "$got"
      fail=1
    fi
  }
  check 'plain feat' ok 'feat: add thing'
  check 'scoped fix' ok 'fix(domain): reject reopen from draft'
  check 'breaking marker' ok 'feat(api)!: change payload'
  check 'every allowed type' ok 'refactor: x'
  check 'missing description' reject 'feat:'
  check 'unknown type' reject 'feature: add thing'
  check 'no colon' reject 'added a thing'
  check 'capital sentence style' ok     'feat: Added a thing'
  exit "$fail"
fi

msg_file="${1:-}"
if [[ -n "$msg_file" && -f "$msg_file" ]]; then
  msg=$(head -n 1 "$msg_file")
elif [[ ! -t 0 ]]; then
  msg=$(head -n 1)
else
  printf 'usage: %s <commit-msg-file> | echo "msg" | %s\n' "$0" "$0" >&2
  exit 2
fi

case "$msg" in
Merge\ * | Revert\ *) exit 0 ;; # git-generated subjects
esac

if echo "$msg" | grep -Eq "$REGEX"; then
  exit 0
fi
printf 'commit message rejected: "%s"\n' "$msg" >&2
printf 'Conventional Commit required: type(scope)?!: description\n' >&2
printf 'types: %s\n' "${TYPES//|/, }" >&2
exit 1
