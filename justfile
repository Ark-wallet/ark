# Find the target directory
CARGO_TARGET := `cargo metadata --format-version 1 --no-deps | jq -r '.target_directory'`
JUSTFILE_DIR := justfile_directory()
export ASPD_EXEC := CARGO_TARGET / "debug" / "aspd"
export BARK_EXEC := CARGO_TARGET / "debug" / "bark"

precheck CHECK:
	bash contrib/prechecks.sh {{CHECK}}
prechecks:
	just precheck rust_no_spaces_for_indent
	just precheck unused_aspd_logs

check:
	cargo check --all --tests

checks: prechecks check

check-commits:
	bash contrib/check-commits.sh

build:
	cargo build --workspace

build-codecov:
	RUSTFLAGS="-C instrument-coverage" LLVM_PROFILE_FILE="your-binary-%p-%m.profraw" cargo build --workspace

docker-pull:
	if [ -n "${LIGHTNINGD_DOCKER_IMAGE-""}" ]; then docker pull "$LIGHTNINGD_DOCKER_IMAGE"; fi

alias unit := test-unit
test-unit TEST="":
	cargo test --workspace --exclude ark-testing {{TEST}}

test-unit-codecov TEST="":
	RUST_TEST_THREADS=1 cargo llvm-cov --workspace --exclude ark-testing --no-report {{TEST}}

test-unit-all:
	cargo test --workspace --exclude ark-testing

test-unit-all-codecov:
	RUST_TEST_THREADS=1 cargo llvm-cov --workspace --exclude ark-testing --no-report

alias int := test-integration
test-integration TEST="": build docker-pull
	cargo test --package ark-testing {{TEST}}

test-integration-codecov TEST="": build-codecov docker-pull
	RUST_TEST_THREADS=1 cargo llvm-cov --package ark-testing --no-report {{TEST}}


alias int-esplora := test-integration-esplora
test-integration-esplora TEST="": build docker-pull
	CHAIN_SOURCE=esplora cargo test --package ark-testing {{TEST}}

test-integration-esplora-codecov TEST="": build-codecov docker-pull
	RUST_TEST_THREADS=1 CHAIN_SOURCE=esplora cargo llvm-cov --package ark-testing --no-report {{TEST}}

test-integration-all: build docker-pull
	cargo test --package ark-testing

test-integration-all-codecov: build-codecov docker-pull
	RUST_TEST_THREADS=1 cargo llvm-cov --package ark-testing --no-report

test: test-unit test-integration test-integration-esplora

codecov-report:
	cargo llvm-cov report --html --output-dir "./target/debug/codecov/"

release-aspd:
	cargo build    --release --target x86_64-unknown-linux-gnu --locked --manifest-path aspd/Cargo.toml

release-bark:
	cargo build --release --target x86_64-unknown-linux-gnu         --locked --manifest-path bark/Cargo.toml
	cargo build --release --target x86_64-pc-windows-gnu            --locked --manifest-path bark/Cargo.toml
	cargo zigbuild --release --target aarch64-unknown-linux-gnu     --locked --manifest-path bark/Cargo.toml
	cargo zigbuild --release --target armv7-unknown-linux-gnueabihf --locked --manifest-path bark/Cargo.toml
	cargo zigbuild --release --target x86_64-apple-darwin           --locked --manifest-path bark/Cargo.toml
	cargo zigbuild --release --target aarch64-apple-darwin          --locked --manifest-path bark/Cargo.toml


RUSTDOCSDIR := justfile_directory() / "rustdocs"
# This is opinionated, but doesn't matter. Any page has full search.
DEFAULT_DOCS_PATH := "bark/struct.Wallet.html"

# Generate rustdoc documentation for all crates and dependencies
[unix]
rustdocs:
	mkdir -p {{RUSTDOCSDIR}}
	cargo doc --target-dir {{RUSTDOCSDIR}} --locked --all --lib --examples --document-private-items
	echo "Open Rust docs at file://{{RUSTDOCSDIR}}/doc/{{DEFAULT_DOCS_PATH}}"

[windows]
rustdocs:
	set shell := ["cmd.exe"]
	# Repetitive because I'm currently unable to create a named variable
	# sed is converting C:\path\to\justfile_folder into /c/path/to/justfile_folder
	mkdir -p $(echo "{{JUSTFILE_DIR}}" | sed 's|\\\\|/|g' | sed 's|^\([a-zA-Z]\):|/\L\1|')/rustdocs
	cargo doc --target-dir $(echo "{{JUSTFILE_DIR}}" | sed 's|\\\\|/|g' | sed 's|^\([a-zA-Z]\):|/\L\1|')/rustdocs --locked --all --lib --examples --document-private-items
	echo "Open Rust docs at file://$(echo "{{JUSTFILE_DIR}}" | sed 's|\\\\|/|g' | sed 's|^\([a-zA-Z]\):|/\L\1|')/rustdocs/doc/{{DEFAULT_DOCS_PATH}}"


# cleans most of our crates, doesn't clean grpc gens, they are sometimes slow to build
clean:
	cargo clean -p ark-lib -p ark-testing -p bark-aspd -p bark-client -p bark-json -p aspd-log

# run a single clippy lint
clippy LINT:
	cargo clippy -- -A clippy::all -W clippy::{{LINT}}

default-aspd-config:
	cargo run -p bark-aspd --example dump-default-config
