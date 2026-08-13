# Bini

Bini is a binary package manager.

**Why?**

- Install a binary not provided by your package manager
- Automate download/extract/install workflow
- Benefit from new features & bug fixes as soon as they are released

**Features**

- Install binaries from GitHub releases in a dedicated `bin` folder
- Match your OS and architecture
- Update binaries based on release date

# Usage

```sh
Usage: bini [OPTIONS] [NAME] [COMMAND]

Commands:
  install  Install from GitHub repository [alias: i]
  list     List installed binaries [alias: l]
  update   Update all installed binaries [alias: u]
  remove   Remove installed binary [aliases: r, rm, uninstall, delete]
  help     Print this message or the help of the given subcommand(s)

Arguments:
  [NAME]  The name of the package to install

Options:
      --as <AS_NAME>  Install the binary under a different name
  -f, --force         Force binary replacement
  -h, --help          Print help
  -V, --version       Print version

Examples:
  bini install sharkdp/bat
  bini install burntsushi/ripgrep --as rg
  bini i sharkdp/bat
  bini sharkdp/bat
```

# Installation

**From source**

```sh
cargo install --frozen --git https://github.com/lapwat/bini
```

**Manage bini binary with bini**

```sh
bini install lapwat/bini
cargo uninstall bini
```

# Storage

The install index is stored in:
- `~/.local/state/bini/index.txt` on Linux
- `~/Library/Application Support/bini/index.txt` on macOS

It keeps track of what binaries you have installed, and under what name.

Binaries are stored in:
- `~/.local/share/bini/bin/` on Linux
- `~/Library/Application Support/bini/bin/` on macOS

You may add this folder to your $PATH.

**For bash**

```sh
echo 'export PATH="$PATH:$HOME/.local/share/bini/bin"' >> ~/.bashrc
source ~/.bashrc
```

**For zsh**

```sh
echo 'export PATH="$PATH:$HOME/.local/share/bini/bin"' >> ~/.zshrc
source ~/.zshrc
```

**For mac**

```sh
echo 'export PATH="$PATH:/Users/q/Library/Application Support/bini/bin"' >> ~/.zshrc
source ~/.zshrc
```

# Update strategy

A local binary is considered out-of-date if its modification date is older than the date of the latest GitHub release.

# Related works

[https://github.com/houseabsolute/ubi](https://github.com/houseabsolute/ubi)
