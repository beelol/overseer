#!/bin/sh
# SYNTHETIC Homebrew for the install tests (AC-90): it writes down what it was asked, and for
# `install --cask ollama` puts the synthetic `ollama` program where $BREW_FIXTURE_TARGET says.
echo "$@" >> "$BREW_FIXTURE_LOG"
if [ "$1" = "install" ] && [ "$2" = "--cask" ] && [ "$3" = "ollama" ]; then
  mkdir -p "$(dirname "$BREW_FIXTURE_TARGET")"
  cp "$BREW_FIXTURE_PROGRAM" "$BREW_FIXTURE_TARGET"
  chmod +x "$BREW_FIXTURE_TARGET"
  exit 0
fi
echo "the fixture only installs the ollama cask" >&2
exit 1
