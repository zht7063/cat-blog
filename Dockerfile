# 构建：docker build -t cat-blog .（docs/v1.0-spec.md §10.2）
# 构建镜像的 Rust 版本和 rust-toolchain.toml 一致，升级时两处一起改。
FROM rust:1.100-slim-trixie AS build
WORKDIR /src
COPY . .
ENV SQLX_OFFLINE=true
RUN cargo build --release --locked --bin cat-blog

FROM debian:trixie-slim
RUN useradd --system --uid 10001 --user-group --home-dir /data --no-create-home cat-blog \
    && install -d -o cat-blog -g cat-blog -m 700 /data
COPY --from=build /src/target/release/cat-blog /usr/local/bin/cat-blog
USER cat-blog
WORKDIR /data
ENV DATABASE_PATH=/data/cat-blog.db \
    BIND_ADDR=0.0.0.0:3000
VOLUME /data
EXPOSE 3000
CMD ["cat-blog", "serve"]
