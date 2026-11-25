#!/usr/bin/env bash
# Renders a social card for X at 2000x1124 (16:9).
#
#   socials/render.sh socials/release/2.17.2.json      -> socials/out/release-2.17.2.png
#   socials/render.sh socials/integration/vercel.json  -> socials/out/integration-vercel.png
#
# The template is the card.html next to the JSON file. Noora's tokens.css comes
# from the Hex package version pinned in web/mix.lock, and a provider logo URL
# (the `logo` field) is downloaded, both into socials/.cache/. Needs curl, jq,
# tar and agent-browser (preinstalled in Amp orbs; `npm i -g agent-browser`).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
data=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
template=$(dirname "$data")
name=$(basename "$template")-$(basename "$data" .json)
cache=$here/.cache
mkdir -p "$cache/logos" "$here/out"

noora=$(sed -n 's/.*"noora": {:hex, :noora, "\([^"]*\)".*/\1/p' "$here/../web/mix.lock")
if [[ ! -f $cache/noora-tokens.css || $(cat "$cache/noora-version" 2>/dev/null) != "$noora" ]]; then
  work=$(mktemp -d)
  curl -fsSL "https://repo.hex.pm/tarballs/noora-$noora.tar" | tar -x -C "$work"
  tar -xzf "$work/contents.tar.gz" -C "$work" priv/static/tokens.css
  mv "$work/priv/static/tokens.css" "$cache/noora-tokens.css"
  echo "$noora" > "$cache/noora-version"
  rm -rf "$work"
fi

card=$(jq -c . "$data")
logo=$(jq -r '.logo // empty' "$data")
if [[ $logo == http* ]]; then
  file=$(basename "$template")-$(basename "$data" .json).${logo##*.}
  [[ -f $cache/logos/$file ]] || curl -fsSL "$logo" -o "$cache/logos/$file"
  card=$(jq -c --arg logo "../.cache/logos/$file" '.logo = $logo' <<<"$card")
fi

# The page sits next to card.html so its relative paths resolve, with the data
# inlined so it renders from file:// without fetch().
page=$(mktemp --suffix=.html -p "$template")
trap 'rm -f "$page"' EXIT
sed "s|<script>|<script>window.CARD = $card;|" "$template/card.html" > "$page"
agent-browser open "file://$page" >/dev/null
agent-browser set viewport 1000 562 2 >/dev/null
agent-browser wait 500 >/dev/null
agent-browser screenshot "$here/out/$name.png"
