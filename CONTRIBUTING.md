# Contributing to FIDM

Thanks for considering a change to FIDM. The project is an early prototype, so focused fixes, documentation improvements, and small feature proposals are easiest to review.

## Before you start

- Search existing issues and pull requests for related work.
- For substantial changes, open an issue describing the use case and proposed approach before investing in a large patch.
- Keep changes focused and explain user-visible behavior in the pull request description.

## Development setup

Install Rust with rustup and the Linux packages listed in the README. Then run:

```sh
cargo fmt --all -- --check
cargo check
cargo run
```

Please include relevant manual verification steps in your pull request. Do not commit build output, local databases, or downloaded files.

## Pull requests

- Use a descriptive title and explain the problem the change addresses.
- Mention platform-specific behavior and any limitations.
- Update the README or other documentation when behavior or setup changes.
- Keep dependency changes purposeful and explain why they are needed.
- Be respectful and follow the project Code of Conduct.
