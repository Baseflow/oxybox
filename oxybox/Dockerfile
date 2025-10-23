# Dockerfile for building the Oxybox application using Rust and musl
FROM clux/muslrust:stable

# Set the environment variables for the build
# We're using the reqwest unstable feature HTTP3
ENV RUSTFLAGS='--cfg reqwest_unstable'
RUN mkdir /source
WORKDIR /source
COPY ./Cargo.toml .
COPY ./src/ ./src/
COPY ./protos ./protos/
COPY ./build.rs .
RUN cargo build --release
RUN strip ./target/x86_64-unknown-linux-musl/release/oxybox

FROM scratch
COPY --from=0 /source/target/x86_64-unknown-linux-musl/release/oxybox /
CMD ["./oxybox"]
