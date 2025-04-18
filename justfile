example-base:
    cargo build
    (cd ./target/debug && ./base-host.exe)
example-rpc:
    cargo build
    (cd ./target/debug && ./rpc-service.exe)