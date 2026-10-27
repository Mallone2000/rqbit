# Development guide

## Rust

Nothing special here. I run with

    RQBIT_HTTP_BASIC_AUTH_USERPASS='developer:use-a-long-local-password' RQBIT_HTTP_API_INSECURE_COOKIES=true make devserver

## Web UI

Start the server

    RQBIT_HTTP_BASIC_AUTH_USERPASS='developer:use-a-long-local-password' RQBIT_HTTP_API_INSECURE_COOKIES=true make devserver

Run Web UI dev

    make webui-dev

## Desktop app

Stop the devserver, otherwise ports will conflict.

Install deps

    cargo install tauri-cli
    make webui-deps

Run tauri dev

    cargo tauri dev

The HTTP API always requires credentials, including local development and read-only
mode. The insecure-cookie flag is for local HTTP development only; deployments
should use HTTPS and the default Secure cookies. `make testserver` also requires
`RQBIT_HTTP_BASIC_AUTH_USERPASS`. Desktop HTTP access reads the same credential
environment variable; without it the optional HTTP listener refuses to serve.
