FROM clux/muslrust
ENV RUSTFLAGS='--cfg reqwest_unstable'
RUN mkdir /source
WORKDIR /source
COPY ./Cargo.toml .
COPY ./Cargo.lock .
COPY ./src/ ./src/
COPY ./protos / ./protos/
COPY ./build.rs .
RUN cargo build --release
RUN strip ./target/x86_64-unknown-linux-musl/release/oxybox

FROM scratch
COPY --from=0 /source/target/x86_64-unknown-linux-musl/release/oxybox /
CMD ["./oxybox"]
