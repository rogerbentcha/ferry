#!/usr/bin/env bash
# Tests of install.sh on this machine: a CI runner (Ubuntu 22.04 or 26.04,
# x86_64 or arm64, with Docker and systemd, and sudo without a password),
# against the release assets in DIR (.github/workflows/release.yml). It
# changes the machine: run it only on a runner that is thrown away after the
# job.
#
#   .github/scripts/install.test.sh DIR
#
# 1. The cases that must stop before any change, and the archive of each CPU.
# 2. A first install, next to a container that must keep running.
# 3. A rerun, which changes nothing.
# 4. An upgrade, which keeps the account and the settings.
# 5. HTTPS settings, and a unit file of your own, which a rerun keeps.
set -euo pipefail

dist=$(cd "$1" && pwd)
script=$dist/install.sh
# The archive of this runner's CPU: uname -m gives x86_64 or aarch64, as the names do.
archive=ferry-$(uname -m)-unknown-linux-gnu.tar.gz
work=$(mktemp -d)
supported='Ferry installs on Ubuntu 22.04, 24.04 and 26.04, Debian 12 and 13, on x86_64 and arm64 (aarch64).'
api=http://127.0.0.1:7878
# The system of this runner, as the script names it: "Ubuntu 26.04.1 LTS".
# shellcheck disable=SC1091
os_name=$(. /etc/os-release && echo "$PRETTY_NAME")

fail() {
  echo "::error::$*"
  exit 1
}
ok() { echo "ok - $*"; }

# run_install [VAR=VALUE...] [-- ARGS...]: install.sh as root, against $dist.
# Its output goes to $work/out, its exit code to $code.
run_install() {
  local env=()
  while [ $# -gt 0 ] && [ "$1" != -- ]; do
    env+=("$1")
    shift
  done
  [ $# -eq 0 ] || shift
  echo "::group::sudo ${env[*]} sh install.sh $*"
  code=0
  # shellcheck disable=SC2024 # the output file belongs to the runner, not to root
  sudo env FERRY_RELEASE_URL="file://$dist" "${env[@]}" sh "$script" "$@" > "$work/out" 2>&1 || code=$?
  cat "$work/out"
  echo "::endgroup::"
}

has() { grep -qF -- "$1" "$work/out" || fail "the output has no '$1'"; }

# expect_stop TEXT: the script failed with TEXT, and the server has nothing of Ferry.
expect_stop() {
  [ "$code" -ne 0 ] || fail "the script did not stop"
  has "$1"
  ! id ferry > /dev/null 2>&1 || fail "the user ferry exists"
  for path in /usr/local/bin/ferryd /usr/local/bin/ferry /etc/systemd/system/ferryd.service /var/lib/ferry /etc/ferry; do
    [ ! -e "$path" ] || fail "$path exists"
  done
}

expect_success() {
  [ "$code" -eq 0 ] || fail "the script failed with exit code $code"
  systemctl is-active --quiet ferryd || fail "ferryd is not active"
  systemctl is-enabled --quiet ferryd || fail "ferryd does not start at boot"
  curl -fsS "$api/healthz" > /dev/null || fail "$api/healthz does not answer"
  sudo cmp -s "$work/release/ferryd" /usr/local/bin/ferryd || fail "/usr/local/bin/ferryd is not the one of the release"
  sudo cmp -s "$work/release/ferry" /usr/local/bin/ferry || fail "/usr/local/bin/ferry is not the one of the release"
  has "ssh -N -L 7878:127.0.0.1:7878 "
  [ "$(docker inspect -f '{{.State.Running}}' ferry-test-keep)" = true ] || fail "the container ferry-test-keep stopped"
  [ "$(systemctl show -p MainPID --value docker)" = "$docker_pid" ] || fail "Docker restarted"
}

# fake_uname CPU: a folder with a uname that says CPU to `uname -m`, to put
# first in PATH.
fake_uname() {
  mkdir -p "$work/uname-$1"
  cat > "$work/uname-$1/uname" << EOF
#!/bin/sh
[ "\$1" = -m ] && { echo $1; exit 0; }
exec /usr/bin/uname "\$@"
EOF
  chmod 755 "$work/uname-$1/uname"
  echo "$work/uname-$1"
}

# mode PATH: "owner mode" of a file.
mode() { sudo stat -c '%U %a' "$1"; }

ferryd_pid() { systemctl show -p MainPID --value ferryd; }

mkdir "$work/release"
tar -xzf "$dist/$archive" -C "$work/release"

echo "## 1. The cases that must stop"

out=$(docker run --rm -v "$script:/install.sh:ro" fedora:41 sh /install.sh 2>&1) && fail "the script ran on Fedora"
grep -qF "$supported" <<< "$out" ||
  fail "on Fedora, the output names no supported system: $out"
ok "Fedora: stops and names the supported systems"

# A 32-bit ARM server and a RISC-V server.
for cpu in armv7l riscv64; do
  run_install "PATH=$(fake_uname "$cpu"):$PATH"
  expect_stop "$supported"
  has "This server runs $os_name on"
  has "on $cpu. The script changed nothing."
  ok "$cpu: stops, names the supported systems and changes nothing"
done

# The archive of each CPU name: from a release with no files, the script
# stops at its first download, and the error names the archive.
mkdir "$work/empty"
for pair in x86_64:x86_64 aarch64:aarch64 arm64:aarch64; do
  name=${pair%%:*}
  want=ferry-${pair#*:}-unknown-linux-gnu.tar.gz
  run_install "PATH=$(fake_uname "$name"):$PATH" "FERRY_RELEASE_URL=file://$work/empty"
  expect_stop "cannot download file://$work/empty/$want"
  ok "uname -m $name: picks $want"
done

code=0
sh "$script" > "$work/out" 2>&1 || code=$?
expect_stop "run the script as root, or with sudo"
ok "without root: stops and changes nothing"

# A server with Docker 24: a docker that says so, first in PATH.
mkdir "$work/old-docker"
cat > "$work/old-docker/docker" << 'EOF'
#!/bin/sh
[ "$1" = version ] && { echo 24.0.7; exit 0; }
exec /usr/bin/docker "$@"
EOF
chmod 755 "$work/old-docker/docker"
run_install "PATH=$work/old-docker:$PATH"
expect_stop "Ferry needs Docker 27.3 or newer. This server has Docker 24.0.7."
ok "Docker 24: stops and changes nothing"

run_install -- --acme-email you@example.com
expect_stop "--acme-email needs --base-domain"
ok "--acme-email without --base-domain: stops and changes nothing"

mkdir "$work/bad"
cp "$dist/$archive" "$work/bad/"
printf '%064d  %s\n' 0 "$archive" > "$work/bad/SHA256SUMS"
run_install "FERRY_RELEASE_URL=file://$work/bad"
expect_stop "does not match its checksum in SHA256SUMS. The script installed nothing."
ok "a bad checksum: stops and changes nothing"

echo "## 2. A first install"

docker run -d --name ferry-test-keep busybox:stable sleep 3600 > /dev/null
docker_pid=$(systemctl show -p MainPID --value docker)
run_install
expect_success
has "Create your account"
code_file=$(sudo cat /var/lib/ferry/setup_code)
has "$api/setup?code=$code_file"
[ "$(ps -o user= -p "$(ferryd_pid)")" = ferry ] || fail "ferryd does not run as the user ferry"
[ "$(mode /var/lib/ferry)" = "ferry 700" ] || fail "/var/lib/ferry is $(mode /var/lib/ferry), not ferry 700"
[ "$(mode /etc/ferry)" = "ferry 700" ] || fail "/etc/ferry is $(mode /etc/ferry), not ferry 700"
[ "$(mode /etc/ferry/ferryd.env)" = "ferry 600" ] || fail "ferryd.env is $(mode /etc/ferry/ferryd.env), not ferry 600"
ok "a first install: ferryd runs under systemd, the setup link and the SSH tunnel are printed, Docker kept"

echo "## 3. A rerun"

pid=$(ferryd_pid)
run_install
expect_success
has "$api/setup?code=$code_file"
[ "$(ferryd_pid)" = "$pid" ] || fail "a rerun with nothing new restarted ferryd"
ok "a rerun: changes nothing, ferryd keeps running"

echo "## 4. An upgrade"

# The account, and a setting of your own: the upgrade must keep both.
curl -fsS -o /dev/null -H 'Content-Type: application/json' \
  -d "{\"email\":\"ci@example.com\",\"password\":\"install-test\",\"code\":\"$code_file\"}" "$api/api/v1/auth/setup"
echo 'FERRY_BUILD_CONCURRENCY=1' | sudo tee -a /etc/ferry/ferryd.env > /dev/null
# An older ferryd: other bytes, so the script must replace it.
cp /usr/local/bin/ferryd "$work/older"
printf 'older' >> "$work/older"
sudo install -m 755 "$work/older" /usr/local/bin/.ferryd.older
sudo mv -f /usr/local/bin/.ferryd.older /usr/local/bin/ferryd
sudo systemctl restart ferryd
for _ in $(seq 60); do curl -fsS "$api/healthz" > /dev/null 2>&1 && break; sleep 1; done
pid=$(ferryd_pid)

run_install -- --base-domain apps.example.com
expect_success
[ "$(ferryd_pid)" != "$pid" ] || fail "the upgrade did not restart ferryd"
has "Open $api in your browser and sign in."
has "apps.example.com"
for line in FERRY_BUILD_CONCURRENCY=1 FERRY_BASE_DOMAIN=apps.example.com FERRY_PROXY_ADDR=0.0.0.0:80; do
  sudo grep -qx "$line" /etc/ferry/ferryd.env || fail "ferryd.env has no line $line"
done
sudo /usr/local/bin/ferryd status --data-dir /var/lib/ferry | grep -qF 'apps.example.com' ||
  fail "ferryd does not use the base domain apps.example.com"
ok "an upgrade: new binaries, ferryd restarted, the account and the settings kept"

echo "## 5. HTTPS settings, and a unit of your own"

run_install -- --acme-email you@example.com
expect_success
for line in FERRY_ACME_EMAIL=you@example.com FERRY_HTTPS_ADDR=0.0.0.0:443 FERRY_BASE_DOMAIN=apps.example.com; do
  sudo grep -qx "$line" /etc/ferry/ferryd.env || fail "ferryd.env has no line $line"
done
ok "--acme-email: HTTPS settings in ferryd.env, ferryd runs"

# Without the first line of the script, the unit is yours.
sudo sed -i 1d /etc/systemd/system/ferryd.service
cp /etc/systemd/system/ferryd.service "$work/own.service"
run_install
expect_success
cmp -s "$work/own.service" /etc/systemd/system/ferryd.service || fail "a rerun changed a unit of your own"
has "/etc/systemd/system/ferryd.service: yours, kept as it is"
ok "a unit of your own: a rerun keeps it"
