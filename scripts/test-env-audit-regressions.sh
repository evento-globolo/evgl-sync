#!/usr/bin/env bash
# Negative and positive env-check tests run in an isolated Git worktree fixture.
# Never create a plaintext secret or decryption key in the source checkout.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
fixture="$(mktemp -d)"
trap 'rm -rf -- "$fixture"' EXIT
mkdir -p "$fixture/.just" "$fixture/env/enc" "$fixture/keyless-config" "$fixture/keyless-home"
cp "$root/justfile" "$fixture/justfile"
cp "$root/.just/env.just" "$fixture/.just/env.just"
cp "$root/.just/check-sops-envelope.py" "$fixture/.just/check-sops-envelope.py"
cp "$root/.gitignore" "$fixture/.gitignore"
git -C "$fixture" init -q
git -C "$fixture" add justfile .just/env.just .just/check-sops-envelope.py .gitignore
git -C "$fixture" -c user.name=env-audit-test -c user.email=env-audit-test@example.invalid commit -qm "isolated fixture"

run_check() (
  cd "$fixture"
  env -u SOPS_AGE_KEY -u SOPS_AGE_KEY_FILE \
    HOME="$fixture/keyless-home" XDG_CONFIG_HOME="$fixture/keyless-config" just env-check
)

log="$fixture/check.log"
expect_denied() {
  local case_name="$1" expected="$2"
  if run_check >"$log" 2>&1; then
    echo "FAIL: audit accepted $case_name" >&2
    exit 1
  fi
  if ! grep -Fq -- "$expected" "$log"; then
    echo "FAIL: audit rejected $case_name for the wrong reason:" >&2
    cat "$log" >&2
    exit 1
  fi
  echo "PASS: rejected $case_name"
}

# Positive control: this must succeed or every negative test could pass for
# an unrelated invocation failure (missing just, bad imports, missing Git).
run_check >"$log" 2>&1 || { cat "$log" >&2; exit 1; }
test ! -e "$fixture/env/dec" && test ! -L "$fixture/env/dec" \
  || { echo "FAIL: read-only audit created env/dec" >&2; exit 1; }
echo "PASS: keyless, read-only clean fixture"

# Every secret-path component must reject symlink substitution.
rmdir "$fixture/env/enc"
ln -s "$fixture/keyless-home" "$fixture/env/enc"
expect_denied "env/enc symlink" "env/enc must not be a symlink"
unlink "$fixture/env/enc"
rmdir "$fixture/env"
ln -s "$fixture/keyless-home" "$fixture/env"
expect_denied "env root symlink" "env must not be a symlink"
unlink "$fixture/env"
mkdir -p "$fixture/env/enc"

ln -s "$fixture/keyless-home" "$fixture/env/dec"
expect_denied "env/dec symlink" "env/dec must not be a symlink"
unlink "$fixture/env/dec"

: >"$fixture/env/dec"
expect_denied "env/dec file masquerading as directory" "env/dec must be a directory"
rm "$fixture/env/dec"

printf 'TEST_ONLY_VALUE=not-a-real-secret\n' >"$fixture/.env"
git -C "$fixture" add -f .env
expect_denied "force-staged plaintext dotenv" "plaintext env files are tracked"
git -C "$fixture" reset -q HEAD -- .env
rm "$fixture/.env"

# Committing a private-key-shaped path must be rejected even if it was
# force-added against .gitignore. This is a fake fixture, never a real key.
printf 'FIXTURE-NOT-A-REAL-AGE-KEY\n' >"$fixture/keys.txt"
git -C "$fixture" add -f keys.txt
expect_denied "force-staged private-key filename" "possible private keys tracked"
git -C "$fixture" reset -q HEAD -- keys.txt
rm "$fixture/keys.txt"

# Check fail-closed behavior if a required ignore rule is removed.
sed -i '/^env\/dec\/$/d' "$fixture/.gitignore"
expect_denied "missing mandatory ignore rule" "missing .gitignore rule: env/dec/"
cp "$root/.gitignore" "$fixture/.gitignore"

printf 'TEST_ONLY_VALUE=plaintext-not-ciphertext\n' >"$fixture/env/enc/dev.env.enc"
expect_denied "plaintext inside ciphertext directory" "is NOT sops-encrypted"
rm "$fixture/env/enc/dev.env.enc"

printf '%s\n' \
  'TEST_ONLY_VALUE=ENC[AES256_GCM,data:fixture,iv:fixture,tag:fixture,type:str]' \
  'map_recipient=fixture-one' \
  'map_recipient=fixture-two' >"$fixture/env/enc/dev.env.enc"
expect_denied "ciphertext missing SOPS MAC" "has no sops MAC"
printf '%s\n' \
  'TEST_ONLY_VALUE=ENC[AES256_GCM,data:fixture,iv:fixture,tag:fixture,type:str]' \
  'sops_mac=fixture' \
  'map_recipient=fixture-one' >"$fixture/env/enc/dev.env.enc"
expect_denied "only one recovery recipient" "needs >= 2"
rm "$fixture/env/enc/dev.env.enc"


# Adversarial keyless cases discovered during audit: refuse ciphertext-file
# symlinks, comment-only AES strings, forged MACs and duplicate age recipients.
printf 'fixture-only' >"$fixture/keyless-home/target-file"
ln -s "$fixture/keyless-home/target-file" "$fixture/env/enc/dev.env.enc"
expect_denied "symlinked encrypted file" "is a symlink or not a regular file"
unlink "$fixture/env/enc/dev.env.enc"

printf '%s\n' \
  '#ENC[AES256_GCM,data:fixture,iv:fixture,tag:fixture,type:str]' \
  'sops_mac=ENC[AES256_GCM,data:fixture,iv:fixture,tag:fixture,type:str]' \
  'sops_age__list_0__map_recipient=age1fixtureone' \
  'sops_age__list_1__map_recipient=age1fixturetwo' >"$fixture/env/enc/dev.env.enc"
expect_denied "comment-only ciphertext" "is NOT sops-encrypted"

printf '%s\n' \
  'TEST_ONLY_VALUE=ENC[AES256_GCM,data:fixture,iv:fixture,tag:fixture,type:str]' \
  'sops_mac=not-an-encrypted-mac' \
  'sops_age__list_0__map_recipient=age1fixtureone' \
  'sops_age__list_1__map_recipient=age1fixturetwo' >"$fixture/env/enc/dev.env.enc"
expect_denied "malformed SOPS MAC" "has no sops MAC"

printf '%s\n' \
  'TEST_ONLY_VALUE=ENC[AES256_GCM,data:fixture,iv:fixture,tag:fixture,type:str]' \
  'sops_mac=ENC[AES256_GCM,data:fixture,iv:fixture,tag:fixture,type:str]' \
  'sops_age__list_0__map_recipient=age1samefixture' \
  'sops_age__list_1__map_recipient=age1samefixture' >"$fixture/env/enc/dev.env.enc"
expect_denied "duplicate recovery recipients" "needs >= 2"
rm "$fixture/env/enc/dev.env.enc"


# Fail closed when Git metadata disappears; the old ls-files pipeline masked it.
mv "$fixture/.git" "$fixture/.git-hidden"
expect_denied "missing Git checkout index" "requires a readable Git checkout and index"
mv "$fixture/.git-hidden" "$fixture/.git"

# Actual encrypted fixture: the parser must admit the canonical SOPS envelope
# without decrypting or providing keys.
cp "$root/env/enc/dev.env.enc" "$fixture/env/enc/dev.env.enc"
run_check >"$log" 2>&1 || { cat "$log" >&2; exit 1; }
echo "PASS: structurally valid ciphertext without decryption"
rm "$fixture/env/enc/dev.env.enc"

# An encrypted marker must not conceal an adjacent unencrypted dotenv value.
cp "$root/env/enc/dev.env.enc" "$fixture/env/enc/dev.env.enc"
printf '%s\n' 'PUBLIC_TEST_VALUE=plaintext-fixture-only' >>"$fixture/env/enc/dev.env.enc"
expect_denied "plaintext adjacent to encrypted value" "invalid SOPS AES-GCM envelope for PUBLIC_TEST_VALUE"
rm "$fixture/env/enc/dev.env.enc"

# Duplicate metadata keys are ambiguous to readers and must be rejected.
cp "$root/env/enc/dev.env.enc" "$fixture/env/enc/dev.env.enc"
printf '%s\n' 'sops_version=3.11.0' >>"$fixture/env/enc/dev.env.enc"
expect_denied "duplicate SOPS version" "duplicate dotenv key sops_version"
rm "$fixture/env/enc/dev.env.enc"

# Corrupt one AES-GCM authentication tag in a ciphertext-only fixture.
cp "$root/env/enc/dev.env.enc" "$fixture/env/enc/dev.env.enc"
python3 - "$fixture/env/enc/dev.env.enc" <<'PY'
from pathlib import Path
import re
import sys
path = Path(sys.argv[1])
lines = path.read_text().splitlines(keepends=True)
changed = False
for i, line in enumerate(lines):
    # SOPS can include an encrypted header comment; corrupt a real value.
    if '=ENC[AES256_GCM' in line and not line.startswith(('#', 'sops_')):
        lines[i], n = re.subn(r'tag:[A-Za-z0-9+/=]+', 'tag:INVALID!', line, count=1)
        changed = n == 1
        break
assert changed, "fixture must contain an encrypted dotenv value"
path.write_text(''.join(lines))
PY
expect_denied "malformed AES-GCM tag" "invalid SOPS AES-GCM envelope"
rm "$fixture/env/enc/dev.env.enc"

# Every age recipient must have a wrapped key, not a bare public label.
cp "$root/env/enc/dev.env.enc" "$fixture/env/enc/dev.env.enc"
python3 - "$fixture/env/enc/dev.env.enc" <<'PY'
from pathlib import Path
import sys
path = Path(sys.argv[1])
lines = path.read_text().splitlines(keepends=True)
lines = [line for line in lines if not line.startswith('sops_age__list_1__map_enc=')]
path.write_text(''.join(lines))
PY
expect_denied "recipient missing wrapped key" "missing or malformed age-wrapped data"
rm "$fixture/env/enc/dev.env.enc"


# Additional SOPS metadata-smuggling negative cases, backported after reviewing
# the stronger Dioxus parser. No private keys or plaintext secrets are used.
mutate_metadata_case() {
  local scenario="$1" expected="$2"
  cp "$root/env/enc/dev.env.enc" "$fixture/env/enc/dev.env.enc"
  python3 - "$fixture/env/enc/dev.env.enc" "$scenario" <<'PY'
from pathlib import Path
import sys

path, scenario = Path(sys.argv[1]), sys.argv[2]
lines = path.read_text().splitlines(keepends=True)

def set_key(name, value):
    matches = [i for i, line in enumerate(lines) if line.startswith(name + "=")]
    assert len(matches) == 1, name
    lines[matches[0]] = name + "=" + value + "\n"

if scenario == "commented_plaintext":
    lines.append("# CLOUD_SECRET=fixture-data-only\n")
elif scenario == "unknown_sops_metadata":
    lines.append("sops_hidden_plaintext=fixture-data-only\n")
elif scenario == "malformed_timestamp":
    set_key("sops_lastmodified", "yesterday")
elif scenario == "timezone_missing":
    set_key("sops_lastmodified", "2026-11-07T12:00:00")
elif scenario == "unexpected_suffix":
    set_key("sops_unencrypted_suffix", "PASSWORD")
else:
    raise AssertionError("unknown metadata mutation")
path.write_text("".join(lines))
PY
  expect_denied "$scenario" "$expected"
  rm "$fixture/env/enc/dev.env.enc"
}

mutate_metadata_case commented_plaintext "plaintext dotenv assignment in ciphertext comment"
mutate_metadata_case unknown_sops_metadata "unexpected SOPS metadata key sops_hidden_plaintext"
mutate_metadata_case malformed_timestamp "missing or invalid SOPS modification timestamp"
mutate_metadata_case timezone_missing "missing or invalid SOPS modification timestamp"
mutate_metadata_case unexpected_suffix "unexpected SOPS unencrypted suffix"

run_check >"$log" 2>&1 || { cat "$log" >&2; exit 1; }
test ! -e "$fixture/env/dec" && test ! -L "$fixture/env/dec" \
  || { echo "FAIL: final audit created env/dec" >&2; exit 1; }
echo "PASS: clean keyless control plus twenty-four adversarial denial cases (symlinks, plaintext, private key, ignore rules, ciphertext shape)"
