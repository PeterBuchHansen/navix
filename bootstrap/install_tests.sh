#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALLER="${ROOT_DIR}/bootstrap/install"

pass_count=0
fail() {
  echo "FAIL: $*" >&2
  exit 1
}

assert_file_contains() {
  local file="$1"
  local expected="$2"
  if ! grep -Fq "${expected}" "${file}"; then
    fail "Expected '${expected}' in ${file}"
  fi
}

assert_equals() {
  local got="$1"
  local want="$2"
  if [[ "${got}" != "${want}" ]]; then
    fail "Expected '${want}', got '${got}'"
  fi
}

run_test() {
  local name="$1"
  shift
  echo "Running ${name}"
  "$@"
  pass_count=$((pass_count + 1))
}

mk_exec() {
  local path="$1"
  local body="$2"
  cat > "${path}" <<EOF
#!/usr/bin/env bash
${body}
EOF
  chmod +x "${path}"
}

test_flat_archive() {
  local td
  td="$(mktemp -d)"

  mkdir -p "${td}/src" "${td}/bin"
  mk_exec "${td}/src/navix" 'echo flat'
  tar -C "${td}/src" -czf "${td}/flat.tar.gz" navix

  "${INSTALLER}" --archive "${td}/flat.tar.gz" --bin-dir "${td}/bin"

  [[ -x "${td}/bin/navix" ]] || fail "navix not installed from flat archive"
  assert_equals "$("${td}/bin/navix")" "flat"
  rm -rf "${td}"
}

test_nested_archive() {
  local td
  td="$(mktemp -d)"

  local nested_dir
  nested_dir="${td}/src/navix-v0.3.0-x86_64-unknown-linux-gnu"
  mkdir -p "${nested_dir}" "${td}/bin"
  mk_exec "${nested_dir}/navix" 'echo nested'
  tar -C "${td}/src" -czf "${td}/nested.tar.gz" "navix-v0.3.0-x86_64-unknown-linux-gnu"

  "${INSTALLER}" --archive "${td}/nested.tar.gz" --bin-dir "${td}/bin"

  [[ -x "${td}/bin/navix" ]] || fail "navix not installed from nested archive"
  assert_equals "$("${td}/bin/navix")" "nested"
  rm -rf "${td}"
}

test_missing_binary_fails() {
  local td
  td="$(mktemp -d)"

  mkdir -p "${td}/src" "${td}/bin"
  echo "missing" > "${td}/src/README.txt"
  tar -C "${td}/src" -czf "${td}/missing.tar.gz" README.txt

  set +e
  "${INSTALLER}" --archive "${td}/missing.tar.gz" --bin-dir "${td}/bin" > "${td}/out.log" 2>&1
  local code=$?
  set -e

  [[ ${code} -ne 0 ]] || fail "missing-binary archive should fail"
  assert_file_contains "${td}/out.log" "Error: navix binary not found in archive"
  rm -rf "${td}"
}

test_force_mode_reinstalls() {
  local td
  td="$(mktemp -d)"

  mkdir -p "${td}/src1" "${td}/src2" "${td}/bin"
  mk_exec "${td}/src1/navix" 'echo first'
  mk_exec "${td}/src2/navix" 'echo second'
  tar -C "${td}/src1" -czf "${td}/first.tar.gz" navix
  tar -C "${td}/src2" -czf "${td}/second.tar.gz" navix

  "${INSTALLER}" --archive "${td}/first.tar.gz" --bin-dir "${td}/bin"
  assert_equals "$("${td}/bin/navix")" "first"

  "${INSTALLER}" --archive "${td}/second.tar.gz" --bin-dir "${td}/bin"
  assert_equals "$("${td}/bin/navix")" "first"

  "${INSTALLER}" --force --archive "${td}/second.tar.gz" --bin-dir "${td}/bin"
  assert_equals "$("${td}/bin/navix")" "second"
  rm -rf "${td}"
}

run_test "flat archive path works" test_flat_archive
run_test "nested archive path works" test_nested_archive
run_test "archive missing binary fails with clear message" test_missing_binary_fails
run_test "force mode reinstalls even when navix exists" test_force_mode_reinstalls

echo "All ${pass_count} installer tests passed."
