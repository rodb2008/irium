# Contributing to Irium

Thank you for your interest in contributing to the Irium blockchain! We welcome contributions from the community.

## 🌟 Ways to Contribute

### 1. Run a Node
Help secure the network by running a full node:
```bash
python3 scripts/irium-node.py
```

### 2. Mine Blocks
Contribute hash power to the network:
```bash
python3 scripts/irium-miner.py
```

### 3. Report Issues
Found a bug or have a suggestion? [Open an issue](https://github.com/iriumlabs/irium/issues/new)

### 4. Submit Code
- Fork the repository
- Create a feature branch (`git checkout -b feature/your-feature`)
- Make your changes
- Write tests if applicable
- Commit with clear messages (`git commit -S -m "Add feature"`)
- Push to your fork (`git push origin feature/your-feature`)
- Open a Pull Request

## 📋 Code Guidelines

### Python Code Style
- Follow PEP 8 style guide
- Use type hints where possible
- Write docstrings for functions and classes
- Keep functions focused and small

### Commit Messages
- Use clear, descriptive commit messages
- Sign your commits with SSH or GPG
- Reference issues when applicable (`Fixes #123`)

### Pull Requests
- Describe what your PR does
- Link to related issues
- Ensure all tests pass
- Keep PRs focused on a single feature/fix

## 🔒 Security

If you discover a security vulnerability:
- **DO NOT** open a public issue
- Contact the maintainers via the project issue tracker or security process.
- Include details and reproduction steps
- We'll respond within 48 hours

## 🧪 Testing

Before submitting code:
```bash
# Test the node
python3 scripts/irium-node.py --test

# Test wallet functionality
python3 scripts/irium-wallet-proper.py new
python3 scripts/irium-wallet-proper.py balance
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
- **Security**: use the private reporting process or reach maintainers via the issue tracker

## 📜 License

By contributing, you agree that your contributions will be licensed under the same license as the project (see LICENSE file).

## 🙏 Thank You

Every contribution, no matter how small, helps make Irium better!

---

**Happy Contributing! 🚀**
