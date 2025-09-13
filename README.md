# Starr

Starr is a modern SSH client and PuTTY alternative, designed to provide a seamless and powerful experience for secure remote connections. It features built-in support for WinSCP, making file transfers as smooth as with PuTTY. The included plink module ensures full compatibility with WinSCP, so you can use Starr as a drop-in replacement for PuTTY in your workflow.

Starr is a modular, GPU-intensive application written in Rust, featuring a core library, a graphical user interface (GUI), and a plink module. The project is structured as a multi-crate workspace, designed for high-performance tasks and extensibility.

## Features

- **Modular Architecture:**
  - `core`: Contains the main logic and reusable components.
  - `gui`: Provides a graphical user interface for user interaction.
  - `plink`: Additional module for extended functionality.
- **Rust-Powered Performance:**
  - Utilizes Rust's safety and speed for demanding workloads.
- **GPU-Intensive Operations:**
  - Designed to leverage GPU resources for computation-heavy tasks.

## Getting Started

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable recommended)
- A modern GPU and up-to-date drivers
- Windows (primary target, other OSes may work but are untested)

### Build Instructions

1. Clone the repository:
   ```sh
   git clone <repo-url>
   cd starr
   ```
2. Build the project:
   ```sh
   cargo build --release
   ```
3. Run the GUI:
   ```sh
   cargo run -p gui --release
   ```

## Project Structure

```
crates/
  core/   # Core library
  gui/    # Graphical user interface
  plink/  # Plink module
```

## Known Issues

- **High GPU Usage:**
  - The application is currently very GPU-intensive. Running it may cause high GPU load and increased power consumption.
- **Send Functionality Not Implemented:**
  - The main sending function is not operational yet. This is a major missing feature and is under development.
- **Stability:**
  - The application is in an early stage. Crashes and unexpected behavior may occur.
- **Platform Support:**
  - Only tested on Windows. Other platforms are not officially supported.

## Roadmap

- [ ] Implement the main sending functionality
- [ ] Optimize GPU usage
- [ ] Improve cross-platform support
- [ ] Enhance stability and error handling

## Contributing

Contributions are welcome! Please open issues or pull requests to help improve the project.

## License

This project is licensed under the MIT License.

---

> **Warning:**
> This software is in active development. Use at your own risk. High GPU usage may impact system performance.
