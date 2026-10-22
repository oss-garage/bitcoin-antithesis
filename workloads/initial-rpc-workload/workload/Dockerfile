FROM rust:1.85-bookworm AS builder

# Build capnproto from source (required by bitcoin-capnp-types build script)
RUN apt-get update && apt-get install -y curl g++ && rm -rf /var/lib/apt/lists/* && \
    curl -fsSL https://capnproto.org/capnproto-c++-1.1.0.tar.gz | tar xz && \
    cd capnproto-c++-1.1.0 && ./configure --prefix=/usr/local && make -j$(nproc) && make install && \
    cd / && rm -rf capnproto-c++-1.1.0

WORKDIR /build

# Copy the workload source
COPY Cargo.toml Cargo.toml
COPY src src

# Build all binaries in release mode
RUN cargo build --release

# Final image
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create Antithesis test directory
RUN mkdir -p /opt/antithesis/test/v1/main

# Copy test driver binaries
COPY --from=builder /build/target/release/parallel_driver_tx_simple /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/parallel_driver_tx_cross_node /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/parallel_driver_tx_sendmany /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/parallel_driver_tx_rbf /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/parallel_driver_tx_addr_types /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/parallel_driver_tx_raw /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/parallel_driver_tx_consolidate /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/parallel_driver_blocks /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/serial_driver_out_of_order_blocks /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/parallel_driver_invalid_blocks /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/eventually_consistency /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/anytime_gettxoutsetinfo /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/anytime_getblocktemplate /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/parallel_driver_prioritise_tx /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/anytime_abandontransaction /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/anytime_loadwallet /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/anytime_echo /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/anytime_unloadwallet /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/anytime_pruneblockchain /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/anytime_wallet_amounts /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/anytime_supply_bound /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/serial_driver_tx_bulkdata /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/parallel_driver_ipc_echo /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/parallel_driver_ipc_mining_info /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/parallel_driver_ipc_block_template /opt/antithesis/test/v1/main/
COPY --from=builder /build/target/release/parallel_driver_ipc_check_block /opt/antithesis/test/v1/main/

# Copy health checker to root (not a test driver)
COPY --from=builder /build/target/release/health_checker /health_checker

# Make binaries executable
RUN chmod +x /opt/antithesis/test/v1/main/* /health_checker

# Default command - do nothing, let Antithesis orchestrate
CMD ["sleep", "infinity"]
