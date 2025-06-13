# Rustow

**Rustow** is a modern Rust implementation of GNU Stow, a symlink farm manager. It helps you organize and manage configuration files (dotfiles) and software packages by creating symbolic links from a target directory to files stored in separate package directories.

## 🌟 Features

- **Full GNU Stow Compatibility**: Supports all major GNU Stow features and command-line options
- **Safe Operation**: Two-phase execution (scan and action) prevents filesystem corruption
- **Dotfiles Management**: Special `--dotfiles` support for managing hidden configuration files
- **Advanced Conflict Resolution**: Comprehensive conflict detection with `--override` and `--defer` options
- **Flexible Ignore Patterns**: Support for local, global, and built-in ignore patterns
- **Tree Folding**: Intelligent directory structure optimization
- **Dry Run Mode**: Preview operations before execution with `--simulate`
- **Verbose Logging**: Detailed output with configurable verbosity levels
- **File Adoption**: Migrate existing files into Stow packages with `--adopt`
- **Cross-Platform**: Works on Unix-like systems with symlink support

## 🚀 Quick Start

### Basic Usage

```bash
# Stow a package (create symlinks)
rustow mypackage

# Stow multiple packages
rustow package1 package2 package3

# Unstow a package (remove symlinks)
rustow -D mypackage

# Restow a package (unstow then stow)
rustow -R mypackage

# Dry run (preview operations)
rustow -n mypackage

# Verbose output
rustow -v mypackage
```

### Directory Structure

Rustow expects a specific directory structure:

```
/path/to/stow/           # Stow directory
├── package1/            # Package directory
│   ├── bin/
│   │   └── program1
│   └── lib/
│       └── libfile.so
├── package2/
│   └── etc/
│       └── config.conf
└── dotfiles/            # Dotfiles package
    ├── dot-bashrc       # Becomes .bashrc when --dotfiles is used
    ├── dot-vimrc        # Becomes .vimrc when --dotfiles is used
    └── dot-config/      # Becomes .config/ when --dotfiles is used
        └── git/
            └── config
```

When you run `rustow package1` from `/path/to/stow/`, it creates symlinks in the target directory (by default, the parent directory `/path/to/`):

```
/path/to/                # Target directory
├── bin/
│   └── program1 -> stow/package1/bin/program1
└── lib/
    └── libfile.so -> stow/package1/lib/libfile.so
```

## 📖 Command Line Options

### Basic Operations

- `rustow PACKAGE...` - Stow packages (create symlinks)
- `-D, --delete` - Unstow packages (remove symlinks)
- `-R, --restow` - Restow packages (unstow then stow)

### Directory Options

- `-t DIR, --target=DIR` - Set target directory (default: parent of stow dir)
- `-d DIR, --dir=DIR` - Set stow directory (default: current directory)

### Special Features

- `--dotfiles` - Enable dot- prefix processing for dotfiles
- `--adopt` - Move conflicting files into stow directory
- `--no-folding` - Disable tree folding optimization

### Conflict Resolution

- `--override=REGEXP` - Force override files matching pattern
- `--defer=REGEXP` - Skip files matching pattern

### Output Control

- `-n, --simulate` - Dry run mode (show what would be done)
- `-v, --verbose[=LEVEL]` - Increase verbosity (0-5)

## 📋 Examples

### Managing Dotfiles

```bash
# Set up dotfiles with the --dotfiles option
cd ~/dotfiles
rustow --dotfiles --target=~ vim bash git

# This creates:
# ~/.vimrc -> ~/dotfiles/vim/dot-vimrc
# ~/.bashrc -> ~/dotfiles/bash/dot-bashrc
# ~/.config/git/config -> ~/dotfiles/git/dot-config/git/config
```

### System Package Management

```bash
# Install packages to /usr/local
cd /usr/local/stow
sudo rustow --target=/usr/local myprogram

# Dry run to see what would happen
rustow -n --target=/usr/local myprogram
```

### Handling Conflicts

```bash
# Override existing files
rustow --override="\.bashrc" mybash

# Defer conflicting files
rustow --defer="\.vimrc" myvim

# Adopt existing files into stow package
rustow --adopt mypackage
```

## 🗂️ Ignore Patterns

Rustow supports three levels of ignore patterns:

1. **Package-local** (`.stow-local-ignore` in package directory)
2. **Global** (`~/.stow-global-ignore` in home directory)  
3. **Built-in defaults**

### Built-in Ignore Patterns

By default, Rustow ignores:
- Version control directories (`.git`, `.svn`, `CVS`, etc.)
- Editor backup files (`*~`, `#*#`, `.#*`)
- Documentation files (`README*`, `LICENSE*`, `COPYING`)
- Ignore files themselves (`.gitignore`, `.stow-local-ignore`)

### Custom Ignore Patterns

Create `.stow-local-ignore` in a package directory:

```
# Ignore all .org files in package root
^/.*\.org$

# Ignore temp directories anywhere
temp

# Ignore specific files
build.log
```

## 🧪 Development

### Architecture

Rustow is built with a modular architecture:

- **CLI Module**: Command-line argument parsing with `clap`
- **Config Module**: Configuration management and validation
- **Stow Module**: Core stow/unstow logic with two-phase execution
- **FS Utils Module**: File system operations abstraction
- **Ignore Module**: Pattern matching for ignore functionality
- **Dotfiles Module**: Dot-prefix processing for dotfiles
- **Error Module**: Comprehensive error handling with `thiserror`

### Running Tests

```bash
# Run all tests
cargo test

# Run integration tests
cargo test --test integration_tests

# Run with verbose output
cargo test -- --nocapture

# Run specific test module
cargo test ignore::tests
```

### Code Quality

```bash
# Format code
cargo fmt

# Check formatting
cargo fmt -- --check

# Run linter
cargo clippy

# Run linter with strict warnings
cargo clippy -- -D warnings

# Security audit (requires cargo-audit)
cargo audit

# Check for problematic dependencies (requires cargo-deny)
cargo deny check
```

## 🤝 Contributing
### Getting Started

1. Fork the repository
2. Create a feature branch
3. Write tests for your feature
4. Implement the feature
5. Ensure all tests pass
6. Submit a pull request

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- GNU Stow project for the original implementation and inspiration
- The Rust community for excellent crates and tooling
- Contributors who help improve this project

## 📚 Further Reading

- [GNU Stow Manual](https://www.gnu.org/software/stow/manual/stow.html)
- [Dotfiles Management Best Practices](https://dotfiles.github.io/)
- [Symlink Farm Management](https://en.wikipedia.org/wiki/Stow_(software))
