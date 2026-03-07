.PHONY: all
all:
	@cargo build
	@cargo build --release

.PHONY: test
test:
	cargo nextest run --release
	cargo nextest run

.PHONY: clean
clean:
	@cargo clean

.PHONY: artifacts
artifacts:
	@gh release view --json assets --jq '.assets[] | "\(.name)\t\(.size)\t\(.downloadCount)"' | column -t -N NAME,SIZE,DOWNLOADS
