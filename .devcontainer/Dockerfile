# syntax=docker/dockerfile:1
# Docker selects the host architecture; supported hosts are amd64 and arm64.
FROM node:24.20.0-trixie

SHELL ["/bin/bash", "-o", "pipefail", "-c"]
ARG TARGETARCH
ARG EMSDK_VERSION=3.1.72
ARG BINARYEN_VERSION=133
ARG CARGO_NDK_VERSION=4.1.2
# Must equal the `wasm-bindgen` pin in crates/epdf-web/Cargo.toml.
ARG WASM_BINDGEN_VERSION=0.2.127
ARG SWIFTLY_VERSION=1.1.4
ARG SWIFT_VERSION=6.3.1
# swiftly has no Debian 13 entry and swift.org publishes no trixie toolchain;
# the bookworm build runs on trixie's newer glibc.
ARG SWIFT_PLATFORM=debian12
ARG CODEX_VERSION=latest
ARG CLAUDE_CODE_VERSION=latest

RUN case "$TARGETARCH" in amd64|arm64) ;; *) echo "Unsupported architecture: $TARGETARCH" >&2; exit 1 ;; esac \
    && apt-get update \
    && apt-get install -y --no-install-recommends \
       build-essential clang libclang-dev lld cmake ninja-build pkg-config \
       libssl-dev python3 python3-venv git git-lfs gh curl ca-certificates \
       unzip zip xz-utils jq ripgrep fd-find less sudo shellcheck \
       gnupg tzdata libicu-dev libcurl4-openssl-dev libedit-dev libsqlite3-dev \
       libncurses-dev libpython3-dev libxml2-dev uuid-dev libstdc++-12-dev \
    && rm -rf /var/lib/apt/lists/* \
    && echo 'node ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/node \
    && chmod 0440 /etc/sudoers.d/node

# Read the package-manager and Rust pins from the checkout at image build time.
COPY package.json rust-toolchain.toml /opt/embedpdf-pins/
RUN npm install -g "$(node -p "require('/opt/embedpdf-pins/package.json').packageManager")"

RUN git clone --depth 1 --branch "$EMSDK_VERSION" https://github.com/emscripten-core/emsdk.git /opt/emsdk \
    && /opt/emsdk/emsdk install "$EMSDK_VERSION" \
    && /opt/emsdk/emsdk activate "$EMSDK_VERSION" \
    && chown -R node:node /opt/emsdk
RUN case "$TARGETARCH" in amd64) binaryen_arch=x86_64 ;; arm64) binaryen_arch=aarch64 ;; esac \
    && curl -fsSL --retry 3 \
      "https://github.com/WebAssembly/binaryen/releases/download/version_${BINARYEN_VERSION}/binaryen-version_${BINARYEN_VERSION}-${binaryen_arch}-linux.tar.gz" \
      -o /tmp/binaryen.tar.gz \
    && mkdir /opt/binaryen \
    && tar -xzf /tmp/binaryen.tar.gz -C /opt/binaryen --strip-components=1 \
    && rm /tmp/binaryen.tar.gz

# NDK r29 = 29.0.14206865; checksum from developer.android.com/ndk/downloads.
# Google provides Linux host binaries only for x86-64.
RUN if [ "$TARGETARCH" = amd64 ]; then \
      curl -fsSL --retry 3 https://dl.google.com/android/repository/android-ndk-r29-linux.zip -o /tmp/ndk.zip \
    && echo '87e2bb7e9be5d6a1c6cdf5ec40dd4e0c6d07c30b  /tmp/ndk.zip' | sha1sum -c - \
    && unzip -q /tmp/ndk.zip -d /opt \
    && rm /tmp/ndk.zip; \
    else \
      echo 'Android NDK omitted: Google does not provide a Linux ARM64 host toolchain.'; \
    fi

ENV EMSDK=/opt/emsdk \
    EM_CONFIG=/opt/emsdk/.emscripten \
    EM_BINARYEN_ROOT=/opt/binaryen \
    ANDROID_NDK_HOME=/opt/android-ndk-r29 \
    SWIFTLY_HOME_DIR=/home/node/.local/share/swiftly \
    SWIFTLY_BIN_DIR=/home/node/.local/share/swiftly/bin \
    NPM_CONFIG_PREFIX=/home/node/.npm-global
ENV PATH="/home/node/.cargo/bin:/home/node/.local/share/swiftly/bin:/home/node/.npm-global/bin:/opt/emsdk/upstream/emscripten:/opt/emsdk/upstream/bin:/opt/binaryen/bin:${PATH}"

USER node
WORKDIR /home/node
RUN curl -fsSL --retry 3 https://sh.rustup.rs -o /tmp/install-rust.sh \
    && bash /tmp/install-rust.sh -y --profile minimal --default-toolchain none \
    && rm /tmp/install-rust.sh \
    && rust_version="$(python3 -c 'import tomllib; print(tomllib.load(open("/opt/embedpdf-pins/rust-toolchain.toml", "rb"))["toolchain"]["channel"])')" \
    && rustup toolchain install "$rust_version" --profile minimal --component rustfmt,clippy,rust-src \
    && rustup default "$rust_version" \
    && python3 -c 'import subprocess,tomllib; t=tomllib.load(open("/opt/embedpdf-pins/rust-toolchain.toml", "rb"))["toolchain"]; subprocess.run(["rustup", "target", "add", "--toolchain", t["channel"], *t["targets"]], check=True)' \
    && cargo install --locked cargo-ndk --version "$CARGO_NDK_VERSION" \
    && cargo install --locked wasm-bindgen-cli --version "$WASM_BINDGEN_VERSION"

# swiftly writes ~/.profile so login shells, which reset PATH via /etc/profile,
# still find the toolchain; the ENV above covers non-login shells.
RUN case "$TARGETARCH" in amd64) swiftly_arch=x86_64 ;; arm64) swiftly_arch=aarch64 ;; esac \
    && curl -fsSL --retry 3 \
      "https://download.swift.org/swiftly/linux/swiftly-${SWIFTLY_VERSION}-${swiftly_arch}.tar.gz" \
      -o /tmp/swiftly.tar.gz \
    && mkdir /tmp/swiftly \
    && tar -xzf /tmp/swiftly.tar.gz -C /tmp/swiftly \
    && /tmp/swiftly/swiftly init --assume-yes --skip-install \
       --quiet-shell-followup --platform "$SWIFT_PLATFORM" \
    && rm -rf /tmp/swiftly /tmp/swiftly.tar.gz \
    && "$SWIFTLY_BIN_DIR/swiftly" install "$SWIFT_VERSION" --use --assume-yes

# Installed as the development user so both CLIs can be upgraded without sudo.
ENV PATH="/opt/binaryen/bin:${PATH}"
RUN npm install -g "@openai/codex@${CODEX_VERSION}" "@anthropic-ai/claude-code@${CLAUDE_CODE_VERSION}" \
    && codex --version && claude --version \
    && rustc --version && swift --version && pnpm --version && emcc --version \
    && wasm-opt --version | grep -F "version ${BINARYEN_VERSION}" \
    && wasm-bindgen --version | grep -F "$WASM_BINDGEN_VERSION"

# Login shells (`bash -l`, which VS Code may open) reset PATH in /etc/profile
# and lose the ENV entries above that no ~/.profile hook restores.
RUN printf '%s\n' \
      'export PATH="/home/node/.npm-global/bin:/opt/emsdk/upstream/emscripten:/opt/emsdk/upstream/bin:/opt/binaryen/bin:$PATH"' \
    | sudo tee /etc/profile.d/embedpdf-toolchain.sh >/dev/null \
    && bash -lc 'command -v wasm-opt && command -v emcc && command -v claude'

USER root
ARG DOTNET_SDK_VERSION=10.0.403
# SHA512 values from Microsoft's .NET 10 release metadata, matched to this SDK.
RUN case "$TARGETARCH" in \
      amd64) dotnet_arch=x64; dotnet_sha512=b7b3d8461cf98b07d2552ce9199702fa3ad010ac2671a5f8cd5136f7534e50f6855ba7bbb02b11494563f1d04156276a31cda1103b900e5eecd0d9473dc7b9c5 ;; \
      arm64) dotnet_arch=arm64; dotnet_sha512=0966aa2b70f4677b8319be9df447a78a0dcd6164836425b55a68fd714f4023feb87b4cac21b9a996b79472ad54487cd0adcd2a9dfd570e2b2a1d26423a2baa9b ;; \
      *) echo "Unsupported .NET architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && curl -fsSL --retry 3 \
      "https://builds.dotnet.microsoft.com/dotnet/Sdk/${DOTNET_SDK_VERSION}/dotnet-sdk-${DOTNET_SDK_VERSION}-linux-${dotnet_arch}.tar.gz" \
      -o /tmp/dotnet-sdk.tar.gz \
    && echo "$dotnet_sha512  /tmp/dotnet-sdk.tar.gz" | sha512sum -c - \
    && mkdir /opt/dotnet \
    && tar -xzf /tmp/dotnet-sdk.tar.gz -C /opt/dotnet \
    && rm /tmp/dotnet-sdk.tar.gz \
    && ln -s /opt/dotnet/dotnet /usr/local/bin/dotnet

ENV DOTNET_ROOT=/opt/dotnet \
    DOTNET_CLI_TELEMETRY_OPTOUT=1 \
    DOTNET_NOLOGO=1
ENV PATH="/opt/dotnet:${PATH}"
USER node
RUN test "$(dotnet --version)" = "$DOTNET_SDK_VERSION" \
    && bash -lc 'test "$(command -v dotnet)" = /usr/local/bin/dotnet'

CMD ["sleep", "infinity"]
