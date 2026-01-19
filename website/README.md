# SLIRCd Website

This directory contains the source code for the SLIRCd website and documentation.

## Structure

- `index.html`: The main landing page.
- `assets/`: CSS and images.
- `docs/`: The documentation source (mdBook).

## Running the Website

To view the landing page, simply open `index.html` in your browser.

## Building the Documentation

The documentation is built using [mdBook](https://rust-lang.github.io/mdBook/).

### Prerequisites

1.  Install Rust.
2.  Install mdBook:
    ```bash
    cargo install mdbook
    ```

### Build

1.  Navigate to the `docs` directory:
    ```bash
    cd docs
    ```
2.  Build the book:
    ```bash
    mdbook build
    ```

The generated documentation will be in `docs/book/`.

### Development

To serve the documentation locally with hot-reloading:

```bash
cd docs
mdbook serve --open
```
