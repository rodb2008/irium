# Contributing to Irium

Thank you for your interest in contributing to the Irium blockchain! We welcome contributions from the community.

## 🌟 Ways to Contribute

### 1. Run a Node
Help secure the network by running a full node:
```bash
./target/release/iriumd
```

### 2. Mine Blocks
Contribute hash power to the network:
```bash
export IRIUM_MINER_ADDRESS=<YOUR_IRIUM_ADDRESS>
./target/release/irium-miner
```

### 3. Report Issues
Found a bug or have a suggestion? [Open an issue](https://github.com/iriumlabs/irium/issues/new)

### 4. Submit Code
- Fork the repository
- Create a feature branch (`git checkout -b feature/your-feature`)
- Make your changes
- Write tests if applicable
- Commit with clear messages (`git commit -m "Add feature"`)
- Push to your fork (`git push origin feature/your-feature`)
- Open a Pull Request

## 📋 Code Guidelines

### Rust Code Style
- Follow idiomatic Rust (`rustfmt`, `clippy`)
- Keep modules cohesive (`chain.rs`, `pow.rs`, `wallet.rs`)
- Add focused comments only where needed
- Avoid hardcoding infrastructure (IPs, DNS, credentials)

### Commit Messages
- Use clear, descriptive commit messages
- Reference issues when applicable (`Fixes #123`)

### Pull Requests
- Describe what your PR does
- Link to related issues
- Ensure all tests pass
- Keep PRs focused on a single feature/fix

## 🔒 Security

If you discover a security vulnerability:
- **DO NOT** open a public issue
- Follow the private reporting process in `SECURITY.md`
- Include details and reproduction steps

## 🧪 Testing

Before submitting code:
```bash
source ~/.cargo/env
cargo test --quiet
```

For runtime validation:
```bash
./target/release/iriumd
./target/release/irium-wallet new-address
export IRIUM_MINER_ADDRESS=<YOUR_IRIUM_ADDRESS>
./target/release/irium-miner
```

## 📚 Documentation

Help improve documentation:
- Fix typos or unclear instructions
- Add examples
- Improve API documentation
- Translate to other languages

## 💬 Community

Join the Irium community and connect with other contributors:

- **GitHub Discussions**: https://github.com/iriumlabs/irium/discussions
  - 📢 Announcements - Stay updated with latest news
  - 🙋 Q&A - Get help and answer questions
  - 💡 Ideas - Share and discuss new ideas
  - ⛏️ Mining - Discuss mining setups and optimizations
  - 🛠️ Development - Technical development discussions

- **Issues**: https://github.com/iriumlabs/irium/issues - Bug reports and feature requests
- **Security**: see `SECURITY.md` for private reporting

## 📜 License

By contributing, you agree that your contributions will be licensed under the same license as the project (see LICENSE file).

## 🙏 Thank You

Every contribution, no matter how small, helps make Irium better!

---

**Happy Contributing! 🚀**
