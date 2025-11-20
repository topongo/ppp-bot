FROM rust AS builder

WORKDIR /src
COPY Cargo.toml .

ARG PROFILE=release
ARG CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu
ENV CARGO_BUILD_TARGET=${CARGO_BUILD_TARGET}

RUN if echo ${CARGO_BUILD_TARGET} | grep musl; then apt update && apt install --yes musl-tools; fi

RUN rustup target add ${CARGO_BUILD_TARGET}
RUN mkdir src src/transcript src/bot && \
    echo "fn main() {}" | tee src/main.rs src/transcript/bin.rs src/download.rs src/bot/bin.rs
RUN cargo build --profile ${PROFILE}

COPY src src
RUN cargo build --profile ${PROFILE} --bin ppp_bot
RUN cargo build --profile ${PROFILE} --bin ppp_import

FROM debian:bookworm-slim AS runtime

RUN apt-get -y update && apt-get -y install tini openssl ffmpeg

WORKDIR /app

RUN useradd -m -s /bin/bash -d /home/ppp ppp
RUN chown -R ppp:ppp /home/ppp /app
USER ppp

ARG PROFILE=release
ARG CARGO_BUILD_TARGET=x86_64-unknown-linux-gnu

COPY --from=builder /src/target/${CARGO_BUILD_TARGET}/${PROFILE}/ppp_bot /usr/local/bin/ppp_bot
COPY --from=builder /src/target/${CARGO_BUILD_TARGET}/${PROFILE}/ppp_import /usr/local/bin/ppp_import

ENTRYPOINT ["/usr/bin/tini", "--"]
