# CamPorter

CamPorter is a desktop application built with Tauri, Svelte, and Rust that helps photographers and videographers efficiently import media files from cameras and other removable devices to their computer.

## Features

- **Device Detection**: Automatically detects connected cameras and removable storage devices
- **Media Scanning**: Scans devices for images and videos
- **Thumbnail Generation**: Creates thumbnails for quick preview of media files
- **Selective Import**: Select specific files to import rather than all files
- **Destination Management**: Save and reuse destination folders
- **Progress Tracking**: Real-time progress indicators during file transfers
- **Duplicate Detection**: Identifies files that already exist in the destination

## Getting Started

### Prerequisites

- [Node.js](https://nodejs.org/) (v16 or later)
- [Rust](https://www.rust-lang.org/tools/install) (latest stable)
- [pnpm](https://pnpm.io/installation) (recommended)

### Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/yourusername/CamPorter.git
   cd CamPorter
   ```

2. Install dependencies:
   ```bash
   pnpm install
   ```

3. Run the development version:
   ```bash
   pnpm tauri dev
   ```

### Building for Production

To create a production build for your platform:

```bash
pnpm tauri build
```

This will generate platform-specific installers in the `src-tauri/target/release/bundle` directory.

## Development

CamPorter is built with:

- **[Tauri](https://tauri.app/)**: For creating lightweight, secure desktop applications
- **[Svelte](https://svelte.dev/)**: For the reactive UI
- **[Rust](https://www.rust-lang.org/)**: For the backend logic and file operations

### Project Structure

- `src/` - Svelte frontend code
- `src-tauri/` - Rust backend code
- `static/` - Static assets

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- [Tauri](https://tauri.app/) for making it possible to build lightweight desktop apps
- [Svelte](https://svelte.dev/) for the excellent frontend framework
- All contributors who have helped improve this project
