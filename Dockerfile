FROM rustlang/rust:nightly as planner
WORKDIR /app
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM rustlang/rust:nightly as cacher
WORKDIR /app
RUN cargo install cargo-chef
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

FROM rustlang/rust:nightly as builder
WORKDIR /app
COPY . .
COPY --from=cacher /app/target target
COPY --from=cacher $CARGO_HOME $CARGO_HOME
RUN cargo build --release

FROM rustlang/rust:nightly as runtime
WORKDIR /app
COPY --from=builder /app/target/release/codera1d-server /usr/local/bin

EXPOSE 8000
ENTRYPOINT ["codera1d-server"]
