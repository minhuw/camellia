# Camellia

[![CI](https://github.com/minhuw/camellia/actions/workflows/ci.yml/badge.svg)](https://github.com/minhuw/camellia/actions/workflows/ci.yml)

## Build

```shell
git submodule update --init --recursive
cargo build
cargo test
cargo bench
```

## Examples and Flamegraph

```shell
cargo run --example forward
cargo flamegraph --root --example forward
```