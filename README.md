# Bini

Bini is a binary package manager.

**Why?**

- Benefit from packages new features & bug fixes as soon as they are out
- Download/extract/install everytime you want to try a new package is tedious

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
  update   Update all installed binaries [aliases: u, ]
  remove   Remove installed binary [aliases: r, rm, uninstall]
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

The index of installed binaries is stored in `~/.local/state/bini/index.txt`. It keeps track of what you have installed, and under what name.

Binaries are stored in `~/.local/share/bini/bin/` folder. You may add this folder to your $PATH.

**For bash**

```sh
echo 'export PATH="$HOME/.local/share/bini/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc
```

**For zsh**

```sh
echo 'export PATH="$HOME/.local/share/bini/bin:$PATH"' >> ~/.zshrc
source ~/.zshrc
```

# Update strategy

A local binary is considered out-of-date if its modification date is older than the date of the latest GitHub release.

# Related works

[https://github.com/houseabsolute/ubi](https://github.com/houseabsolute/ubi)
