example-base:
    cargo build
    (cd ./target/debug && ./host.exe)
example-rpc:
    cargo build
    (cd ./target/debug && ./rpc-service.exe)