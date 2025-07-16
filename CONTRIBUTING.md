# Contributing to pg-capture

Thank you for your interest in contributing to pg-capture! This document provides guidelines and instructions for contributing.

## Code of Conduct

By participating in this project, you agree to abide by our Code of Conduct:
- Be respectful and inclusive
- Welcome newcomers and help them get started
- Focus on what is best for the community
- Show empathy towards other community members

## How to Contribute

### Reporting Issues

1. Check if the issue already exists in the [issue tracker](https://github.com/yourusername/pg-capture/issues)
2. If not, create a new issue with:
   - Clear, descriptive title
   - Detailed description of the problem
   - Steps to reproduce
   - Expected vs actual behavior
   - Environment details (OS, PostgreSQL version, Kafka version)
   - Relevant logs or error messages

### Suggesting Features

1. Check the [issue tracker](https://github.com/yourusername/pg-capture/issues) for existing feature requests
2. Open a new issue with the "enhancement" label
3. Describe the feature and its use case
4. Explain why this would be useful to other users

### Pull Requests

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Add tests for new functionality
5. Ensure all tests pass (`cargo test`)
6. Run formatting and linting:
   ```bash
   cargo fmt
   cargo clippy -- -D warnings
   ```
7. Commit your changes with a descriptive message
8. Push to your fork
9. Open a Pull Request

## Development Setup

### Prerequisites

- Rust 1.75 or later
- Docker and Docker Compose
- PostgreSQL 14+ (for local testing)
- Apache Kafka 3.0+ (for local testing)

### Local Development

1. Clone the repository:
   ```bash
   git clone https://github.com/yourusername/pg-capture.git
   cd pg-capture
   ```

2. Start test infrastructure:
   ```bash
   docker-compose up -d postgres kafka
   ```

3. Build the project:
   ```bash
   cargo build
   ```

4. Run tests:
   ```bash
   # Unit tests
   cargo test
   
   # Integration tests
   ./tests/run_integration_tests.sh
   ```

### Project Structure

```
pg-capture/
├── src/
│   ├── main.rs              # CLI entry point
│   ├── lib.rs               # Library exports
│   ├── config.rs            # Configuration handling
│   ├── error.rs             # Error types
│   ├── replicator.rs        # Main replication logic
│   ├── checkpoint.rs        # Checkpoint management
│   ├── postgres/            # PostgreSQL replication
│   │   ├── connection.rs    # Replication connection
│   │   ├── decoder.rs       # PgOutput decoder
│   │   └── types.rs         # Type conversions
│   └── kafka/               # Kafka integration
│       ├── producer.rs      # Kafka producer
│       └── serializer.rs    # JSON serialization
├── tests/                   # Integration tests
├── examples/                # Example code
└── config/                  # Configuration examples
```

## Testing

### Writing Tests

- Add unit tests in the same file as the code using `#[cfg(test)]`
- Place integration tests in the `tests/` directory
- Use descriptive test names that explain what is being tested
- Test both success and failure cases

Example:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        // Test implementation
    }
}
```

### Running Tests

```bash
# All tests
cargo test

# Specific test
cargo test test_message_serialization

# Integration tests only
cargo test --test integration_test -- --ignored

# With debug output
RUST_LOG=debug cargo test -- --nocapture
```

## Code Style

### Rust Guidelines

- Follow standard Rust naming conventions
- Use `rustfmt` for formatting
- Address all `clippy` warnings
- Write documentation for public APIs
- Use meaningful variable and function names
- Prefer explicit error handling over `unwrap()`

### Commit Messages

Follow the conventional commits specification:

```
type(scope): short description

Longer explanation if necessary. Wrap at 72 characters.

Fixes #123
```

Types:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, etc.)
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Test additions or changes
- `chore`: Build process or auxiliary tool changes

### Documentation

- Update README.md for user-facing changes
- Add inline documentation for complex code
- Update configuration examples if needed
- Include examples for new features

## Performance Considerations

When contributing performance-critical code:

1. Benchmark before and after changes
2. Use `cargo bench` for micro-benchmarks
3. Profile with tools like `perf` or `flamegraph`
4. Consider memory allocation patterns
5. Document performance characteristics

## Release Process

Releases are automated via GitHub Actions when a tag is pushed:

1. Update version in `Cargo.toml`
2. Update CHANGELOG.md
3. Commit changes: `git commit -m "chore: prepare v0.2.0"`
4. Tag release: `git tag v0.2.0`
5. Push: `git push origin main --tags`

## Getting Help

- Join our [Discord server](https://discord.gg/example)
- Check the [documentation](https://github.com/yourusername/pg-capture/wiki)
- Ask questions in [GitHub Discussions](https://github.com/yourusername/pg-capture/discussions)

## License

By contributing, you agree that your contributions will be licensed under the MIT License.