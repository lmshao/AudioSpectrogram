# AudioSpectrogram

AudioSpectrogram is a powerful audio spectrogram generator that supports various audio formats and produces high-quality spectrograms. Built with Rust, it offers cross-platform support and runs on Windows, macOS, and Linux.

## Features

- Cross-platform support (Windows, macOS, Linux)
- Drag-and-drop support on Windows
- Multiple audio format support: WAV, MP3, FLAC, OGG, AAC, etc.
- High-quality spectrogram generation using Turbo colormap
- Automatic multi-channel audio processing (mixed to mono)
- Precise time and frequency scales
- Complete dB scale display (-120dB to 0dB)
- Customizable FFT size and hop size

## Sample Spectrogram

![Sample Spectrogram](resources/sample-spectrogram.jpg)

This is a sample spectrogram generated using our tool, showing OneRepublic's "Apologize" (44.1kHz sampling rate). The spectrogram clearly demonstrates:

- Full frequency range (0-22.05kHz)
- Clear time axis markers
- Precise frequency scaling
- Rich dynamic range display (-120dB to 0dB)

## Requirements

- Rust toolchain (recommended installation via [rustup](https://rustup.rs/))
- Cargo (Rust package manager, included with Rust)
- System requires at least one monospace font:
  - Windows: Consolas
  - macOS: Monaco
  - Linux: DejaVu Sans Mono

## Building

1. Clone the repository:

```bash
git clone https://github.com/lmshao/AudioSpectrogram.git
cd AudioSpectrogram
```

2. Build the project:

```bash
cargo build --release
```

The executable will be available in the `target/release` directory.

## Usage

Basic usage:

```bash
AudioSpectrogram -i input.mp3
```

On Windows, you can simply drag and drop an audio file onto the program icon, and it will automatically generate a spectrogram. This is the easiest way to use the program.

Alternatively, specify the file directly in the command line:

```bash
AudioSpectrogram input.mp3
```

### Command Line Arguments

- `-i, --input <FILE>`: Input audio file path
- `-o, --output <FILE>`: Output image path (optional, defaults to input filename with .png extension)
- `-f, --fft-size <SIZE>`: FFT size (optional, default: 4096)
- `-p, --hop-size <SIZE>`: Hop size (optional, default: half of FFT size)

### Examples

1. Generate spectrogram with default parameters:

```bash
AudioSpectrogram -i music.flac
```

2. Specify output filename:

```bash
AudioSpectrogram -i music.flac -o spectrum.png
```

3. Custom FFT parameters:

```bash
AudioSpectrogram -i music.flac -f 8192 -p 2048
```

### Output Description

The generated spectrogram includes:

- Vertical axis: Frequency scale (kHz)
- Horizontal axis: Time scale (min:sec)
- Right side: dB scale (-120dB to 0dB)
- Color mapping: Using Turbo colormap, red indicates high intensity, blue indicates low intensity

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.
