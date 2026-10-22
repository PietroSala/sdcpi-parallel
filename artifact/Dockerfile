# The artifact image of the TACAS 2027 submission 5092, "A Parallel
# Algorithm for Strategy Synthesis in CPI": the tool sdcpi built from this
# repository, Storm 1.13.0 as the campaign ran it, and the Python of the
# analysis and of the figures. Build from the root of the repository:
#
#     docker build -f artifact/Dockerfile -t sdcpi-tacas27 .
#
# The base image carries both architectures and the build takes the one of
# the machine; --platform linux/amd64 or linux/arm64 asks for the other. The
# record of the artifact carries the image of each, built in this way.
FROM movesrwth/storm:1.13.0

# The toolchains: Rust for the tool, Python for the scripts of the
# analysis and of the figures, unzip and rsync for the archives of the
# records, and the shared libraries of the headless Chrome that kaleido
# drives to write the figures as PDF.
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y \
        --no-install-recommends \
        build-essential curl ca-certificates python3 python3-venv unzip rsync \
        libnss3 libnspr4 libdbus-1-3 libatk1.0-0t64 libatk-bridge2.0-0t64 \
        libcups2t64 libdrm2 libxkbcommon0 libxcomposite1 libxdamage1 \
        libxfixes3 libxrandr2 libgbm1 libasound2t64 libpango-1.0-0 \
        libcairo2 libexpat1 libx11-6 libxcb1 libxext6 fonts-liberation \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo \
    PATH=/opt/venv/bin:/opt/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
        --default-toolchain 1.99.0
# The versions are those that generated the benchmark and the figures of
# the paper. The Chromium that kaleido drives to write the figures is
# fetched at build time through playwright, which carries it for amd64 and
# arm64 alike, and is linked where kaleido looks for it; nothing is fetched
# when the artifact runs.
RUN python3 -m venv /opt/venv \
    && pip install --no-cache-dir \
        numpy==2.3.5 scipy==1.17.1 pandas==2.3.3 matplotlib==3.11.0 \
        PyYAML==6.0.3 plotly==6.9.0 kaleido==1.3.0 playwright==1.64.0 \
    && playwright install chromium \
    && ln -s "$(find /root/.cache/ms-playwright -type f -name chrome \
                -path '*chrome-linux*' | head -1)" /usr/local/bin/chromium \
    && ln -s /usr/local/bin/chromium /usr/local/bin/google-chrome \
    && chromium --version

# The repository, and the tool built from it. The generator of the
# benchmark reads process-impact-benchmarks at the commit the campaign
# pinned: a checkout that does not carry it, as a snapshot of the
# repository does not, clones it here, and the pin is checked either way.
COPY . /artifact/sdcpi-parallel
WORKDIR /artifact/sdcpi-parallel
RUN cd scripts/storm-comparison-ablation \
    && python3 -c 'import setup, sys; sys.exit(setup.fetch_benchmark())'
RUN cargo build --release \
    && install -m 0755 target/release/sdcpi /usr/local/bin/sdcpi

CMD ["bash"]
