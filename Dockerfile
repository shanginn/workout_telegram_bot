FROM rust:slim

WORKDIR /var/app
COPY . /var/app

RUN cargo build --release

ENTRYPOINT ["cargo", "run", "--release"]
