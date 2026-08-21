# Windowsではpowershellを、Linux・macOSではjust既定のシェルを使う。
# `set shell`と違いWindows以外には影響しないため、OSごとに書き換える必要はない。
set windows-shell := ["powershell.exe", "-c"]

help:
    just -l

fmt:
    cargo +nightly fmt --all
