.PHONY: build test run clean release check fmt

build:
	cargo build

test:
	cargo test

run:
	cargo run

release:
	cargo build --release

check:
	cargo check

fmt:
	cargo fmt

lint:
	cargo clippy -- -W warnings

clean:
	cargo clean

install: release
	cp target/release/shako ~/.local/bin/shako

register-shell:
	@if grep -q "$(HOME)/.local/bin/shako" /etc/shells 2>/dev/null; then \
		echo "shako already in /etc/shells"; \
	else \
		echo "$(HOME)/.local/bin/shako" | sudo tee -a /etc/shells; \
		echo "shako added to /etc/shells"; \
		echo "run: chsh -s $(HOME)/.local/bin/shako"; \
	fi
