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

build:
	cargo build --workspace

docker-pull:
	if [ -n "${LIGHTNINGD_DOCKER_IMAGE-""}" ]; then docker pull "$LIGHTNINGD_DOCKER_IMAGE"; fi

alias unit := test-unit
test-unit TEST="":
	cargo test --workspace --exclude ark-testing {{TEST}}

alias int := test-integration
test-integration TEST="": build docker-pull
	cargo test --package ark-testing {{TEST}}

test: test-unit test-integration

RUSTDOCSDIR := justfile_directory() / "rustdocs"
DEFAULT_CRATE := "bark" # This is opinionated, but doesn't matter. Any page has full search.

# Generate rustdoc documentation for all crates and dependencies
[unix]
rustdocs:
	mkdir -p {{RUSTDOCSDIR}}
	cargo doc --target-dir {{RUSTDOCSDIR}} --locked --all --lib --examples --document-private-items
	echo "Open Rust docs at file://{{RUSTDOCSDIR}}/doc/{{DEFAULT_CRATE}}/index.html"

[windows]
rustdocs:
	set shell := ["cmd.exe"]
	# Repetitive because I'm currently unable to create a named variable
	# sed is converting C:\path\to\justfile_folder into /c/path/to/justfile_folder
	mkdir -p $(echo "{{JUSTFILE_DIR}}" | sed 's|\\\\|/|g' | sed 's|^\([a-zA-Z]\):|/\L\1|')/rustdocs
	cargo doc --target-dir $(echo "{{JUSTFILE_DIR}}" | sed 's|\\\\|/|g' | sed 's|^\([a-zA-Z]\):|/\L\1|')/rustdocs --locked --all --lib --examples --document-private-items
	echo "Open Rust docs at file://$(echo "{{JUSTFILE_DIR}}" | sed 's|\\\\|/|g' | sed 's|^\([a-zA-Z]\):|/\L\1|')/rustdocs/doc/{{DEFAULT_CRATE}}/index.html"


# cleans most of our crates, doesn't clean grpc gens, they are sometimes slow to build
clean:
	cargo clean -p ark-lib -p ark-testing -p bark-aspd -p bark-client -p bark-json -p aspd-log

# run a single clippy lint
clippy LINT:
	cargo clippy -- -A clippy::all -W clippy::{{LINT}}
