# For Windows users. If you are linux or macOS user, please delete this line.
set shell := ["powershell.exe", "-c"]

help:
    just -l

fmt:
    cargo +nightly fmt --all
