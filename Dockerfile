# Using multistage build:
# 	https://docs.docker.com/develop/develop-images/multistage-build/
# 	https://whitfin.io/speeding-up-rust-docker-builds/


##########################  BUILD IMAGE  ##########################
# Musl build image to build Conduits statically compiled binary
FROM rustlang/rust:nightly-alpine3.12 as builder

# Don't download Rust docs
RUN rustup set profile minimal

ENV USER "conduit"
#ENV RUSTFLAGS='-C link-arg=-s'

# Install packages needed for building all crates
RUN apk add --no-cache \
        musl-dev \
        openssl-dev \
        pkgconf

# Create dummy project to fetch all dependencies.
# Rebuilds are a lot faster when there are no changes in the
# dependencies.
RUN cargo new --bin /app
WORKDIR /app

# Copy cargo files which specify needed dependencies
COPY ./Cargo.* ./

# Add musl target, as we want to run your project in
# an alpine linux image
RUN rustup target add x86_64-unknown-linux-musl

# Build dependencies and remove dummy project, except
# target folder, as it contains the dependencies
RUN cargo build --release --color=always ; \
    find . -not -path "./target*" -delete

# Now copy and build the real project with the pre-built
# dependencies.
COPY . .
RUN cargo build --release --color=always

########################## RUNTIME IMAGE ##########################
# Create new stage with a minimal image for the actual
# runtime image/container
FROM alpine:3.12

ARG CREATED
ARG VERSION
ARG GIT_REF=HEAD

# Labels according to https://github.com/opencontainers/image-spec/blob/master/annotations.md
# including a custom label specifying the build command
LABEL org.opencontainers.image.created=${CREATED} \
      org.opencontainers.image.authors="Conduit Contributors, weasy@hotmail.de" \
      org.opencontainers.image.title="Conduit" \
      org.opencontainers.image.version=${VERSION} \
      org.opencontainers.image.vendor="Conduit Contributors" \
      org.opencontainers.image.description="A Matrix homeserver written in Rust" \
      org.opencontainers.image.url="https://conduit.rs/" \
      org.opencontainers.image.revision=$GIT_REF \
      org.opencontainers.image.source="https://git.koesters.xyz/timo/conduit.git" \
      org.opencontainers.image.documentation.="" \
      org.opencontainers.image.licenses="AGPL-3.0" \
      org.opencontainers.image.ref.name="" \
      org.label-schema.docker.build="docker build . -t conduit:latest --build-arg CREATED=$(date -u +'%Y-%m-%dT%H:%M:%SZ') --build-arg VERSION=$(grep -m1 -o '[0-9].[0-9].[0-9]' Cargo.toml)"\
      maintainer="weasy@hotmail.de"

# Change some Rocket.rs default configs. They can then
# be changed to different values using env variables.
ENV ROCKET_CLI_COLORS="on"
#ENV ROCKET_SERVER_NAME="conduit.rs"
ENV ROCKET_ENV="production"
ENV ROCKET_ADDRESS=0.0.0.0
ENV ROCKET_PORT=14004
ENV ROCKET_LOG="normal"
ENV ROCKET_DATABASE_PATH="/data/sled"
ENV ROCKET_REGISTRATION_DISABLED="true"
#ENV ROCKET_WORKERS=10

EXPOSE 14004

# Copy config files from context and the binary from
# the "builder" stage to the current stage into folder
# /srv/conduit and create data folder for database
RUN mkdir -p /srv/conduit /data/sled

COPY --from=builder /app/target/release/conduit ./srv/conduit/

# Add www-data user and group with UID 82, as used by alpine
# https://git.alpinelinux.org/aports/tree/main/nginx/nginx.pre-install
RUN set -x ; \
    addgroup -Sg 82 www-data 2>/dev/null ; \
    adduser -S -D -H -h /srv/conduit -G www-data -g www-data www-data 2>/dev/null ; \
    addgroup www-data www-data 2>/dev/null && exit 0 ; exit 1

# Change ownership of Conduit files to www-data user and group
RUN chown -cR www-data:www-data /srv/conduit /data

VOLUME /data

RUN apk add --no-cache \
        ca-certificates

# Set user to www-data
USER www-data
WORKDIR /srv/conduit
ENTRYPOINT [ "/srv/conduit/conduit" ]
