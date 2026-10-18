#!/bin/sh
# Installs Ferry on a fresh Linux server, or upgrades it to the latest
# release: Docker and git when they are missing, ferryd and ferry, a `ferry`
# user and a systemd service. Then it prints the link that creates your
# account and the SSH tunnel that opens it.
#
#   curl -fsSL https://github.com/rogerbentcha/ferry/releases/latest/download/install.sh | sudo sh
#   curl -fsSL .../install.sh | sudo sh -s -- --base-domain apps.example.com --acme-email you@example.com
#
# Ubuntu 22.04 / 24.04 and Debian 12 / 13, x86_64. Every check runs before
# the first change: a server the script refuses is left as it was.
#
# FERRY_RELEASE_URL replaces the release it downloads from (the release
# workflow points it at the assets it just built, with file://).
set -eu
# `su` without `-` on Debian leaves /usr/sbin (useradd) out of PATH.
PATH=$PATH:/usr/local/sbin:/usr/sbin:/sbin

INSTALL_URL=https://github.com/rogerbentcha/ferry/releases/latest/download/install.sh
RELEASE_URL=${FERRY_RELEASE_URL:-https://github.com/rogerbentcha/ferry/releases/latest/download}
ARCHIVE=ferry-x86_64-unknown-linux-gnu.tar.gz
SUPPORTED='Ubuntu 22.04 and 24.04, Debian 12 and 13, on x86_64'
BIN_DIR=/usr/local/bin
DATA_DIR=/var/lib/ferry
CONFIG_DIR=/etc/ferry
ENV_FILE=$CONFIG_DIR/ferryd.env
UNIT_FILE=/etc/systemd/system/ferryd.service
# The first line of the unit this script writes: a rerun replaces only that one.
UNIT_MARK='# Written by the Ferry install script.'

say() { printf '%s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }

# fail MESSAGE [LINE...]: the error, the lines under it, then exit 1.
fail() {
  printf '\nError: %s\n' "$1" >&2
  shift
  for line in "$@"; do printf '  %s\n' "$line" >&2; done
  exit 1
}

usage() {
  cat <<EOF
Install Ferry on this server, or upgrade it to the latest release.

  curl -fsSL $INSTALL_URL | sudo sh -s -- [options]

Options:
  --base-domain DOMAIN  Serve your apps at <name>.DOMAIN, on port 80
  --acme-email EMAIL    Turn on HTTPS on port 443, with certificates from
                        Let's Encrypt (needs a base domain)
  -h, --help            Print this help

Without options, ferryd keeps its defaults: your apps are at
<name>.localhost:8080. A rerun keeps $DATA_DIR and $ENV_FILE.
Supported: $SUPPORTED.
EOF
}

parse_args() {
  base_domain=
  acme_email=
  while [ $# -gt 0 ]; do
    case $1 in
      --base-domain | --acme-email)
        [ $# -ge 2 ] && [ -n "$2" ] || fail "$1 needs a value."
        if [ "$1" = --base-domain ]; then base_domain=$2; else acme_email=$2; fi
        shift 2
        ;;
      --base-domain=*) base_domain=${1#*=}; shift ;;
      --acme-email=*) acme_email=${1#*=}; shift ;;
      -h | --help) usage; exit 0 ;;
      *) usage >&2; fail "unknown option: $1" ;;
    esac
  done
  case $base_domain in
    *[!A-Za-z0-9.-]* | .* | *. | *..*) fail "--base-domain: '$base_domain' is not a domain name." ;;
  esac
  case $acme_email in
    '') ;;
    *[!A-Za-z0-9.@+_%-]* | *@*@* | @* | *@) fail "--acme-email: '$acme_email' is not an email address." ;;
    *@*) ;;
    *) fail "--acme-email: '$acme_email' is not an email address." ;;
  esac
}

# os_field NAME: one field of /etc/os-release.
os_field() {
  # shellcheck disable=SC1091
  (. /etc/os-release && eval "printf '%s' \"\${$1:-}\"")
}

check_system() {
  step "Checking the server"
  arch=$(uname -m)
  os_id=
  os_version=
  os_codename=
  os_name='an unknown system'
  if [ -r /etc/os-release ]; then
    os_id=$(os_field ID)
    os_version=$(os_field VERSION_ID)
    os_codename=$(os_field VERSION_CODENAME)
    os_name=$(os_field PRETTY_NAME)
  fi
  case "$os_id $os_version $arch" in
    "ubuntu 22.04 x86_64" | "ubuntu 24.04 x86_64" | "debian 12 x86_64" | "debian 13 x86_64") ;;
    *) fail "Ferry installs on $SUPPORTED." "This server runs $os_name on $arch. The script changed nothing." ;;
  esac
  [ "$(id -u)" = 0 ] || fail "run the script as root, or with sudo:" "curl -fsSL $INSTALL_URL | sudo sh"
  [ -d /run/systemd/system ] || fail "Ferry runs as a systemd service, and systemd does not run on this server."
  for tool in curl tar sha256sum; do
    command -v "$tool" > /dev/null 2>&1 || fail "the script needs $tool: apt-get install $tool"
  done
  say "$os_name, $arch"
}

# Docker is kept as it is when it is there: the script only checks it.
check_docker() {
  install_docker=
  if ! command -v docker > /dev/null 2>&1; then
    install_docker=1
    say "Docker: not installed. The script installs it from Docker's repository."
    return
  fi
  how="Upgrade Docker from Docker's repository: https://docs.docker.com/engine/install/$os_id/"
  docker_version=$(docker version --format '{{.Server.Version}}' 2> /dev/null) ||
    fail "Docker is installed, but its daemon does not answer." "Start it: systemctl start docker" "Then run the script again."
  major=${docker_version%%.*}
  minor=${docker_version#*.}
  minor=${minor%%.*}
  case $major.$minor in
    *[!0-9.]* | .* | *.) fail "cannot read the version of Docker: '$docker_version'." ;;
  esac
  if [ "$major" -lt 27 ] || { [ "$major" -eq 27 ] && [ "$minor" -lt 3 ]; }; then
    fail "Ferry needs Docker 27.3 or newer. This server has Docker $docker_version." \
      "The script does not change a Docker that is already installed." "$how" "Then run the script again."
  fi
  systemctl cat docker.service > /dev/null 2>&1 ||
    fail "Docker $docker_version does not run as the systemd service docker.service, and ferryd starts after it." \
      "Docker from a snap is not supported. Install Docker from Docker's repository: https://docs.docker.com/engine/install/$os_id/"
  docker buildx version > /dev/null 2>&1 ||
    fail "Docker $docker_version has no buildx plugin. Ferry builds your apps with it." \
      "Install the package docker-buildx-plugin (Docker's repository) or docker-buildx (the docker.io of $os_name)." \
      "Then run the script again."
  getent group docker > /dev/null 2>&1 ||
    fail "Docker $docker_version has no group docker, and the user ferry uses Docker through it."
  say "Docker $docker_version: kept as it is."
}

check_options() {
  if [ -n "$acme_email" ] && [ -z "$base_domain" ] && ! grep -qs '^FERRY_BASE_DOMAIN=' "$ENV_FILE"; then
    fail "--acme-email needs --base-domain: Let's Encrypt gives certificates for a public domain."
  fi
}

# fetch URL FILE
fetch() {
  curl -fsSL --retry 3 -o "$2" "$1" || fail "cannot download $1"
}

# Download and check the release, in a temporary folder: nothing is
# installed yet.
download() {
  step "Downloading Ferry"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT
  trap 'exit 1' HUP INT TERM
  fetch "$RELEASE_URL/$ARCHIVE" "$tmp/$ARCHIVE"
  fetch "$RELEASE_URL/SHA256SUMS" "$tmp/SHA256SUMS"
  # SHA256SUMS also lists install.sh: check the archive only.
  awk -v f="$ARCHIVE" '$2 == f || $2 == "*" f' "$tmp/SHA256SUMS" > "$tmp/archive.sha256"
  [ -s "$tmp/archive.sha256" ] || fail "SHA256SUMS has no checksum for $ARCHIVE. The script installed nothing."
  (cd "$tmp" && sha256sum -c --status archive.sha256) ||
    fail "$ARCHIVE does not match its checksum in SHA256SUMS. The script installed nothing." \
      "Run the script again to download it again."
  tar -xzf "$tmp/$ARCHIVE" -C "$tmp" ferryd ferry
  say "$ARCHIVE: checksum OK"
}

# apt_get ARGS...: no question, and a wait of up to 5 minutes for the lock of
# dpkg, which the automatic updates of a first boot can hold.
apt_get() {
  DEBIAN_FRONTEND=noninteractive LC_ALL=C apt-get -qq -y -o DPkg::Lock::Timeout=300 "$@" < /dev/null
}

# apt-get update does not wait for its own lock: try again for up to 5 minutes.
apt_update() {
  tries=0
  while ! apt_get update 2> "$tmp/apt-update.err"; do
    if ! grep -q 'Could not get lock' "$tmp/apt-update.err"; then
      cat "$tmp/apt-update.err" >&2
      fail "apt-get update failed."
    fi
    [ "$tries" -lt 60 ] ||
      fail "another apt-get still holds the lock of apt after 5 minutes." "Run the script again when it is done."
    [ "$tries" -gt 0 ] || say "Another apt-get runs (the updates of a first boot): waiting for it"
    tries=$((tries + 1))
    sleep 5
  done
}

install_packages() {
  packages=
  command -v git > /dev/null 2>&1 || packages="$packages git"
  command -v ssh > /dev/null 2>&1 || packages="$packages openssh-client"
  if [ -z "$packages" ] && [ -z "$install_docker" ]; then
    return
  fi
  if [ -n "$install_docker" ]; then
    step "Adding Docker's repository"
    # A repository of Docker that is there already stays the only one.
    if ! grep -rqs 'download.docker.com' /etc/apt/sources.list /etc/apt/sources.list.d; then
      install -m 0755 -d /etc/apt/keyrings
      fetch "https://download.docker.com/linux/$os_id/gpg" /etc/apt/keyrings/docker.asc
      chmod a+r /etc/apt/keyrings/docker.asc
      cat > /etc/apt/sources.list.d/docker.sources <<EOF
Types: deb
URIs: https://download.docker.com/linux/$os_id
Suites: $os_codename
Components: stable
Signed-By: /etc/apt/keyrings/docker.asc
EOF
    fi
    packages="$packages docker-ce docker-ce-cli containerd.io docker-buildx-plugin"
  fi
  step "Installing$packages"
  apt_update
  # shellcheck disable=SC2086 # one word per package
  apt_get install $packages > /dev/null
  if [ -n "$install_docker" ]; then
    systemctl enable --now --quiet docker
    say "Docker $(docker version --format '{{.Server.Version}}')"
  fi
}

# The user ferry runs ferryd. It can use Docker through the group docker.
setup_user() {
  step "Setting up the user ferry and its folders"
  if id ferry > /dev/null 2>&1; then
    say "User ferry: already there"
  else
    useradd --system --create-home --home-dir /home/ferry --shell /usr/sbin/nologin --groups docker ferry
  fi
  # Only ferry opens them (DESIGN.md §15). A rerun keeps them as they are.
  for dir in "$DATA_DIR" "$CONFIG_DIR"; do
    [ -d "$dir" ] || install -d -o ferry -g ferry -m 700 "$dir"
  done
  if [ ! -e "$ENV_FILE" ]; then
    cat > "$tmp/ferryd.env" <<EOF
# The settings of ferryd: one FERRY_* environment variable per line.
# ferryd --help lists them. After a change: systemctl restart ferryd
# A rerun of the install script keeps this file.
EOF
    install -o ferry -g ferry -m 600 "$tmp/ferryd.env" "$ENV_FILE"
  fi
  # The public ports go with a public domain. Ports set before stay.
  if [ -n "$base_domain" ]; then
    set_env FERRY_BASE_DOMAIN "$base_domain"
    grep -qs '^FERRY_PROXY_ADDR=' "$ENV_FILE" || set_env FERRY_PROXY_ADDR 0.0.0.0:80
  fi
  if [ -n "$acme_email" ]; then
    set_env FERRY_ACME_EMAIL "$acme_email"
    grep -qs '^FERRY_PROXY_ADDR=' "$ENV_FILE" || set_env FERRY_PROXY_ADDR 0.0.0.0:80
    grep -qs '^FERRY_HTTPS_ADDR=' "$ENV_FILE" || set_env FERRY_HTTPS_ADDR 0.0.0.0:443
  fi
}

# set_env KEY VALUE: the line KEY=VALUE in the env file, in place of the old one.
set_env() {
  grep -qxF "$1=$2" "$ENV_FILE" && return
  grep -v "^$1=" "$ENV_FILE" > "$tmp/ferryd.env" || true
  printf '%s=%s\n' "$1" "$2" >> "$tmp/ferryd.env"
  install -o ferry -g ferry -m 600 "$tmp/ferryd.env" "$ENV_FILE"
  say "$ENV_FILE: $1=$2"
  restart=1
}

install_binaries() {
  step "Installing ferryd and ferry in $BIN_DIR"
  old_version=
  if [ -x "$BIN_DIR/ferryd" ]; then
    old_version=$("$BIN_DIR/ferryd" --version 2> /dev/null) || old_version=
  fi
  for bin in ferryd ferry; do
    cmp -s "$tmp/$bin" "$BIN_DIR/$bin" && continue
    # A rename replaces the file of a ferryd that runs.
    install -m 755 "$tmp/$bin" "$BIN_DIR/.$bin.new"
    mv -f "$BIN_DIR/.$bin.new" "$BIN_DIR/$bin"
    [ "$bin" = ferry ] || restart=1
  done
  new_version=$("$BIN_DIR/ferryd" --version)
  new_version=${new_version#ferryd }
  case $old_version in
    '') say "Ferry $new_version" ;;
    "ferryd $new_version") say "Ferry $new_version: already installed" ;;
    *) say "Ferry ${old_version#ferryd } -> $new_version" ;;
  esac
}

# The unit of Guides -> Production setup, with the settings in the env file.
write_unit() {
  cat > "$tmp/ferryd.service" <<EOF
$UNIT_MARK A rerun replaces this file.
# Put your settings in $ENV_FILE, or delete the first line to keep your changes.
[Unit]
Description=Ferry server
Requires=docker.service
After=docker.service network-online.target
Wants=network-online.target

[Service]
User=ferry
Group=ferry
SupplementaryGroups=docker
# Bind ports 80 and 443 without running as root.
AmbientCapabilities=CAP_NET_BIND_SERVICE
EnvironmentFile=$ENV_FILE
ExecStart=$BIN_DIR/ferryd run --data-dir $DATA_DIR
Restart=always
# Let ferryd drain requests and stop jobs on shutdown.
TimeoutStopSec=60
LimitNOFILE=65536
# When the host runs out of memory, let the kernel kill containers before ferryd.
OOMScoreAdjust=-900

[Install]
WantedBy=multi-user.target
EOF
  if [ -e "$UNIT_FILE" ]; then
    case $(head -n 1 "$UNIT_FILE") in
      "$UNIT_MARK"*) ;;
      *)
        say "$UNIT_FILE: yours, kept as it is"
        if [ -n "$base_domain$acme_email" ]; then
          say "  Its ExecStart flags win over $ENV_FILE."
        fi
        return
        ;;
    esac
  fi
  cmp -s "$tmp/ferryd.service" "$UNIT_FILE" && return
  install -m 644 "$tmp/ferryd.service" "$UNIT_FILE"
  systemctl daemon-reload
  restart=1
}

# Start ferryd (again when something changed), then wait until it serves.
start_service() {
  step "Starting ferryd"
  systemctl enable --quiet ferryd
  if [ -n "$restart" ] || ! systemctl is-active --quiet ferryd; then
    systemctl restart ferryd
  fi
  tries=0
  while :; do
    # ferryd writes its API address in ferryd.json when the API listens.
    # `ferryd status` takes the lock of the data directory for a moment: call
    # it only then, when it cannot meet a ferryd that starts.
    if grep -qs '"api_url"' "$DATA_DIR/ferryd.json"; then
      status=$("$BIN_DIR/ferryd" status --data-dir "$DATA_DIR" 2> /dev/null) || status=
      case $status in *"Create your account"* | *"Connect the CLI"*) break ;; esac
    fi
    tries=$((tries + 1))
    if systemctl is-failed --quiet ferryd || [ "$tries" -ge 90 ]; then
      journalctl -u ferryd -n 30 --no-pager >&2 || true
      fail "ferryd did not start. Its last log lines are above." "All of them: journalctl -u ferryd"
    fi
    sleep 1
  done
}

# The address you reach this server at: the one of your SSH session, else
# the first address of the server. sudo removes SSH_CONNECTION from the
# environment: the processes above this one (sudo, your shell) still have it.
server_address() {
  conn=${SSH_CONNECTION:-}
  pid=$PPID
  depth=0
  while [ -z "$conn" ] && [ "$pid" -gt 1 ] && [ "$depth" -lt 10 ]; do
    conn=$(tr '\0' '\n' 2> /dev/null < "/proc/$pid/environ" | sed -n 's/^SSH_CONNECTION=//p' | head -n 1) || conn=
    pid=$(sed -n 's/^PPid:[[:space:]]*//p' "/proc/$pid/status" 2> /dev/null) || pid=
    [ -n "$pid" ] || pid=1
    depth=$((depth + 1))
  done
  addr=$(say "$conn" | awk '{ print $3 }')
  [ -n "$addr" ] || addr=$(hostname -I 2> /dev/null | awk '{ print $1 }') || addr=
  say "${addr:-your-server}"
}

print_next_steps() {
  api_url=$(say "$status" | sed -n 's/^ *Dashboard + API *: *//p' | head -n 1)
  [ -n "$api_url" ] || api_url=http://127.0.0.1:7878
  apps=$(say "$status" | sed -n 's/^ *Apps *: *//p' | head -n 1)
  link=$(say "$status" | sed -n 's/^ *Create your account: *//p' | head -n 1)
  api=${api_url#*://}
  api=${api%%/*}
  port=${api##*:}
  tunnel="ssh -N -L $port:${api%:*}:$port ${SUDO_USER:-root}@$(server_address)"

  step "Ferry $new_version runs under systemd"
  say "  It starts again after a reboot."
  say "  Apps     : $apps"
  say "  Settings : $ENV_FILE, then: sudo systemctl restart ferryd"
  say "  Logs     : sudo journalctl -u ferryd -f"
  say ""
  if [ -n "$link" ]; then
    say "Create your account. On your computer:"
  else
    say "Open the dashboard. On your computer:"
  fi
  say "  1. Open an SSH tunnel to this server (use the address you connect to):"
  say "       $tunnel"
  if [ -n "$link" ]; then
    say "  2. Open this link in your browser:"
    say "       $link"
  else
    say "  2. Open $api_url in your browser and sign in."
  fi
}

main() {
  restart=
  parse_args "$@"
  check_system
  check_docker
  check_options
  download
  install_packages
  setup_user
  install_binaries
  write_unit
  start_service
  print_next_steps
}

# Last line: a download cut short runs nothing.
main "$@"
