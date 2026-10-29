# Contributing to Face Liveness

Thank you for your interest in contributing to Face Liveness!

Contributions of all kinds are welcome, including bug fixes, documentation improvements, tests, performance improvements, and new functionality.

This document explains how to contribute to the project.

## 1. Before You Start

Before making a change:

1. Check existing issues and pull requests to see whether the problem or feature has already been discussed.
2. For substantial changes, open an issue first to discuss the proposed approach.
3. Keep changes focused and explain why they are needed.

For security-sensitive issues, avoid publicly sharing exploit details or sensitive information. Use a private reporting channel if one is provided by the maintainers.

## 2. Development Environment

The project uses the following tools:

- Rust and Cargo
- Node.js and pnpm
- Python and uv
- PyO3 and Maturin for Python bindings
- napi-rs for Node.js bindings

Install the tools required for the component you intend to modify.

Clone the repository:

```bash
git clone https://github.com/wiseman-umanah/Face-Liveness.git
cd face-liveness
```

Install the dependencies for the component you are working on.

For the Rust workspace:

```bash
cargo build --workspace
```

For the Node.js package:

```bash
cd crates/liveness-node
pnpm install
```

For the Python package:

```bash
cd crates/liveness-py
uv sync
```

## 3. Making Changes

Please follow these guidelines:

- Keep shared functionality in `liveness-core` whenever it can be reused by multiple language bindings.
- Keep Python-specific integration code in `liveness-py` (the `faceliveness` Python package).
- Keep Node.js-specific integration code in `liveness-node` (the `faceliveness` npm package).
- When behaviour changes, update `tests/conformance/cases.json` so every language checks it.
- Avoid duplicating core business logic across language bindings.
- Prefer clear, descriptive names.
- Document public APIs and non-obvious implementation decisions.
- Add or update tests when changing behavior.
- Avoid unrelated changes in the same pull request.

If a change affects a public API, update the relevant documentation and examples.

## 4. Code Quality

Before submitting a pull request, run the checks relevant to your changes.

### Rust formatting

```bash
cargo fmt --all -- --check
```

If formatting changes are needed, apply them with:

```bash
cargo fmt --all
```

### Rust linting

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

### Rust tests

```bash
cargo test --workspace
```

### Node.js

Run the relevant build, test, and lint scripts defined in `crates/liveness-node/package.json`.

For example, if the package defines these scripts:

```bash
pnpm build
pnpm test
pnpm lint
```

Run the scripts that apply to your changes.

### Python

```bash
cd crates/liveness-py
uv sync
uv run pytest
```

Do not claim that a check passes unless you have run it successfully.

## 5. Commit Messages

Write clear commit messages that describe the change.

Examples:

```text
feat(core): add version reporting
fix(node): correct native function export
test(core): add version tests
docs: improve development instructions
refactor(py): simplify module initialization
```

These prefixes are suggestions rather than strict requirements.

## 6. Pull Requests

Before opening a pull request:

- Ensure your changes are focused and understandable.
- Run the relevant formatting, linting, build, and test checks.
- Update documentation where necessary.
- Remove generated artifacts and local configuration files from the changes.
- Explain the motivation and behavior of the change.
- Link any related issue.

A pull request should include:

1. **Summary:** What changed?
2. **Motivation:** Why is the change needed?
3. **Testing:** Which commands or tests did you run?
4. **Compatibility:** Does the change affect existing APIs?

Keep pull requests small enough to review effectively.

## 7. Reporting Bugs

When reporting a bug, include:

- A clear description of the problem.
- Steps to reproduce it.
- Expected behavior.
- Actual behavior.
- Relevant error messages.
- Operating system, Rust, Node.js, and Python versions where applicable.

Do not include passwords, access tokens, private images, or other sensitive information in public issue reports.

## 8. Security and Privacy

Face-related software can involve sensitive personal data.

Do not commit private images, biometric samples, credentials, production data, or other sensitive information.

Use synthetic or appropriately authorized test data when possible.

Any future face-liveness functionality should be evaluated carefully before being used for security-sensitive decisions.

## 9. License

By submitting a contribution, you agree that it will be licensed under the [Apache License 2.0](LICENSE).
