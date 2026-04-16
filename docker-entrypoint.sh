#!/bin/sh
set -eu

APP_USER="${APP_USER:-taskbot}"
APP_GROUP="${APP_GROUP:-taskbot}"
APP_DATA_DIR="${APP_DATA_DIR:-/app/data}"

umask 027
mkdir -p "$APP_DATA_DIR"
chown -R "$APP_USER:$APP_GROUP" "$APP_DATA_DIR"

exec gosu "$APP_USER:$APP_GROUP" "$@"
