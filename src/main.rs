use clap::Parser;
use hound::{SampleFormat, WavReader};
use image::{ImageBuffer, Rgb};
use imageproc::drawing::{draw_line_segment_mut, draw_text_mut};
use rustfft::{FftPlanner, num_complex::Complex};
use rusttype::{Font, Scale};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

mod build_time {
    include!(concat!(env!("OUT_DIR"), "/build_time.rs"));
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input audio file path (supports WAV, MP3, FLAC, OGG, AAC, etc.)
    #[arg(short, long)]
    input: String,

    /// Output spectrogram file path
    #[arg(short, long)]
    output: Option<String>,

    /// FFT size
    #[arg(short, long, default_value_t = 4096)]
    fft_size: usize,

    /// Hop size (defaults to half of FFT size)
    #[arg(short = 'p', long)]
    hop_size: Option<usize>,
}

fn read_audio_samples(path: &str) -> Result<(Vec<f32>, u32), Box<dyn std::error::Error>> {
    // First try to read as WAV using hound for backward compatibility
    if path.to_lowercase().ends_with(".wav") {
        return match read_wav_samples(path) {
            Ok(result) => Ok(result),
            Err(e) => {
                println!("WAV decoding failed, trying generic decoder: {}", e);
                read_generic_audio(path)
            }
        };
    }

    // For other formats, use symphonia
    read_generic_audio(path)
}

fn read_generic_audio(path: &str) -> Result<(Vec<f32>, u32), Box<dyn std::error::Error>> {
    // Create a media source from the file
    let file = File::open(path)?;
    let media_source = MediaSourceStream::new(Box::new(file), Default::default());

    // Create a hint to help the format registry guess what format reader is appropriate
    let mut hint = Hint::new();
    if let Some(extension) = Path::new(path).extension() {
        hint.with_extension(extension.to_str().unwrap_or(""));
    }

    // Use the default options for format and metadata
    let format_opts: FormatOptions = Default::default();
    let metadata_opts: MetadataOptions = Default::default();

    // Probe the media source to determine the format
    let probed = symphonia::default::get_probe().format(
        &hint,
        media_source,
        &format_opts,
        &metadata_opts,
    )?;

    // Get the format reader
    let mut format = probed.format;

    // Find audio track
    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("No valid audio track found. If this is an OGG file, it might contain cover art.")?;

    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);

    // Create a decoder for the track
    let mut decoder = match symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
    {
        Ok(decoder) => decoder,
        Err(e) => {
            println!("Failed to create decoder: {}", e);
            println!("Please ensure the correct codec features are enabled.");
            return Err(e.into());
        }
    };

    let mut merged_samples = Vec::new();

    // Decode the audio packets
    while let Ok(packet) = format.next_packet() {
        // Decode the packet into audio samples
        let decoded = decoder.decode(&packet)?;

        // Get the audio buffer specification
        let spec = *decoded.spec();
        let num_channels = spec.channels.count();

        // Create the sample buffer
        let mut sample_buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);

        // Copy the decoded audio samples into the sample buffer
        sample_buf.copy_interleaved_ref(decoded);

        let samples = sample_buf.samples();

        // Process samples in groups of channels
        for chunk in samples.chunks(num_channels) {
            let mut sum = 0.0;
            let mut count = 0;

            // Only use first two channels if available
            let channels_to_use = num_channels.min(2);
            for channel in 0..channels_to_use {
                if let Some(&sample) = chunk.get(channel) {
                    sum += sample;
                    count += 1;
                }
            }

            if count > 0 {
                merged_samples.push(sum / count as f32);
            }
        }
    }

    Ok((merged_samples, sample_rate))
}

fn read_wav_samples(path: &str) -> Result<(Vec<f32>, u32), hound::Error> {
    let reader = WavReader::open(path)?;
    let sample_rate = reader.spec().sample_rate;
    let sample_format = reader.spec().sample_format;
    let channels = reader.spec().channels as usize;
    println!("{:?}", reader.spec());

    let mut channel_samples: Vec<Vec<f32>> = vec![Vec::new(); channels];

    // First collect all channel data
    match sample_format {
        SampleFormat::Int => {
            let samples: Vec<i32> = reader.into_samples::<i32>().map(|s| s.unwrap()).collect();
            let max_value = samples.iter().fold(0, |a, &b| a.max(b.abs()));

            // Distribute samples to channels
            for (i, &sample) in samples.iter().enumerate() {
                let channel = i % channels;
                channel_samples[channel].push(sample as f32 / max_value as f32);
            }
        }
        SampleFormat::Float => {
            let samples: Vec<f32> = reader.into_samples::<f32>().map(|s| s.unwrap()).collect();

            // Distribute samples to channels
            for (i, &sample) in samples.iter().enumerate() {
                let channel = i % channels;
                channel_samples[channel].push(sample);
            }
        }
    }

    // Merge channels (average of available channels)
    let mut merged_samples = Vec::with_capacity(channel_samples[0].len());
    for i in 0..channel_samples[0].len() {
        let mut sum = 0.0;
        let mut count = 0;

        // Only use first two channels (left and right) if available
        let channels_to_use = channels.min(2);
        for channel in 0..channels_to_use {
            sum += channel_samples[channel][i];
            count += 1;
        }
        merged_samples.push(sum / count as f32);
    }

    Ok((merged_samples, sample_rate))
}

fn compute_spectrum(samples: &[f32], fft_size: usize) -> Vec<f32> {
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    // 1. Apply Hanning window and convert to complex input
    let window: Vec<f32> = (0..fft_size)
        .map(|i| {
            0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size as f32 - 1.0)).cos())
        })
        .collect();
    let mut input: Vec<Complex<f32>> = samples
        .iter()
        .take(fft_size)
        .zip(window.iter())
        .map(|(s, w)| Complex::new(s * w, 0.0))
        .collect();

    // 2. Perform FFT
    fft.process(&mut input);

    // 3. Compute magnitude spectrum
    input[..fft_size / 2].iter().map(|c| c.norm()).collect()
}

fn get_system_font() -> Option<Vec<u8>> {
    let font_path = if cfg!(target_os = "windows") {
        "C:\\Windows\\Fonts\\consola.ttf"
    } else if cfg!(target_os = "macos") {
        "/System/Library/Fonts/Monaco.ttf"
    } else {
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
    };

    std::fs::read(font_path).ok()
}

fn generate_spectrogram(
    samples: &[f32],
    sample_rate: u32,
    fft_size: usize,
    hop_size: usize,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    // Set margins for scale drawing
    let margin_left = 160u32; // Left margin for frequency scale
    let margin_right = 180u32; // Right margin, symmetric with left
    let margin_top = 60u32; // Top margin, symmetric with bottom
    let margin_bottom = 60u32; // Bottom margin for time scale

    // Calculate main plotting area dimensions
    let num_frames = if samples.len() >= fft_size {
        (samples.len() - fft_size) / hop_size + 1
    } else {
        0
    };
    let height = fft_size / 2;
    // println!("num_frames: {}, height: {}", num_frames, height);

    // Calculate colorbar position and dimensions
    let colorbar_x = margin_left + (num_frames as u32) + 40; // Colorbar position
    let colorbar_width = 30u32; // Colorbar width
    let colorbar_height = height as u32; // Colorbar height same as spectrogram
    let min_needed_width = colorbar_x + colorbar_width + 100; // Extra space for dB scale text

    // Create image with margins and colorbar space
    let total_width = min_needed_width;
    let total_height = (height as u32) + margin_top + margin_bottom;
    let mut img = ImageBuffer::from_fn(total_width, total_height, |_, _| Rgb([255, 255, 255]));

    // Store all spectral values to calculate global min/max
    let mut all_magnitudes = Vec::new();
    let gradient = colorgrad::turbo();

    // First calculate all spectral values
    for i in 0..num_frames {
        let start = i * hop_size;
        if start + fft_size > samples.len() {
            break;
        }
        let chunk = &samples[start..start + fft_size];
        let spectrum = compute_spectrum(chunk, fft_size);
        all_magnitudes.extend(spectrum);
    }

    // Draw spectrogram body
    for (x, i) in (0..num_frames).enumerate() {
        let start = i * hop_size;
        if start + fft_size > samples.len() {
            break;
        }
        let chunk = &samples[start..start + fft_size];
        let spectrum = compute_spectrum(chunk, fft_size);

        for (y, &magnitude) in spectrum.iter().enumerate() {
            let db_min = -120.0;
            let db_max = 0.0;
            let denom = db_max - db_min;
            let log_mag = if magnitude > 1e-10 {
                magnitude.log10()
            } else {
                -10.0
            };
            let db_val = log_mag * 20.0;
            let mut normalized = (db_val - db_min) / denom;
            if !normalized.is_finite() {
                normalized = 0.0;
            }
            normalized = normalized.max(0.0).min(1.0);

            let color = gradient.at(normalized as f64).to_rgba8();
            let y_pos = total_height - margin_bottom - (y as u32) - 1;

            // Only draw within valid spectrogram area
            if y_pos >= margin_top && y_pos < (total_height - margin_bottom) {
                img.put_pixel(
                    (x as u32) + margin_left,
                    y_pos,
                    Rgb([color[0], color[1], color[2]]),
                );
            }
        }
    }

    // Load font
    let font_data = get_system_font().expect(
        "Could not find system font. Please ensure at least one monospace font is installed",
    );
    let font = Font::try_from_bytes(&font_data).expect("Invalid font file format");

    // Draw axes
    let black = Rgb([0, 0, 0]);
    // Vertical axis (frequency)
    draw_line_segment_mut(
        &mut img,
        (margin_left as f32, margin_top as f32),
        (margin_left as f32, (total_height - margin_bottom) as f32),
        black,
    );

    // Horizontal axis (time)
    draw_line_segment_mut(
        &mut img,
        (margin_left as f32, (total_height - margin_bottom) as f32),
        (
            (total_width - margin_right) as f32,
            (total_height - margin_bottom) as f32,
        ),
        black,
    );

    // Draw left frequency scale
    draw_frequency_scale(
        &mut img,
        &font,
        margin_left,
        margin_top,
        margin_bottom,
        total_height,
        sample_rate,
        (total_height - margin_top - margin_bottom) as f32,
    );

    // Draw bottom time scale
    draw_time_scale(
        &mut img,
        &font,
        margin_left,
        margin_right,
        margin_bottom,
        total_width,
        total_height,
        samples.len(),
        sample_rate,
        num_frames,
    );

    // Draw colorbar legend on the right
    draw_colorbar_with_scale(
        &mut img,
        &font,
        margin_top,
        colorbar_x,
        colorbar_width,
        colorbar_height,
        &gradient,
    );

    img
}

// Draw left frequency scale
fn draw_frequency_scale(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    font: &Font,
    margin_left: u32,
    margin_top: u32,
    margin_bottom: u32,
    total_height: u32,
    sample_rate: u32,
    height_scale: f32,
) {
    let freq_scale = Scale::uniform(24.0);
    let max_freq = sample_rate as f32 / 2.0;

    // Calculate frequency ticks
    let mut last_drawn_freq = -1000.0; // Initialize to a negative value to ensure the first tick (0kHz) will be drawn

    // Draw ticks starting from 0Hz
    for i in (0..=(max_freq as i32)).step_by(1000) {
        let freq = i as f32;
        // Skip if frequency exceeds maximum
        if freq > max_freq {
            break;
        }

        let y_pos = total_height - margin_bottom - ((freq / max_freq * height_scale) as u32) - 1;

        if y_pos >= margin_top && y_pos < (total_height - margin_bottom) {
            let freq_text = format!("{:.1}kHz", freq / 1000.0);
            draw_text_mut(
                img,
                Rgb([0, 0, 0]),
                50,
                y_pos as i32 - 12,
                freq_scale,
                font,
                &freq_text,
            );
            // Tick marks
            draw_line_segment_mut(
                img,
                (margin_left as f32 - 5.0, y_pos as f32),
                (margin_left as f32, y_pos as f32),
                Rgb([0, 0, 0]),
            );
            last_drawn_freq = freq;
        }
    }

    // Check if we need to draw the highest frequency tick
    // Only draw if the difference from the last drawn tick is >= 1kHz
    if max_freq - last_drawn_freq >= 1000.0 {
        // Draw highest frequency label
        let max_freq_text = format!("{:.1}kHz", max_freq / 1000.0);
        draw_text_mut(
            img,
            Rgb([0, 0, 0]),
            50,
            margin_top as i32 - 12,
            freq_scale,
            font,
            &max_freq_text,
        );
        // Highest frequency tick mark
        draw_line_segment_mut(
            img,
            (margin_left as f32 - 5.0, margin_top as f32),
            (margin_left as f32, margin_top as f32),
            Rgb([0, 0, 0]),
        );
    }
}

// Draw bottom time scale
fn draw_time_scale(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    font: &Font,
    margin_left: u32,
    margin_right: u32,
    margin_bottom: u32,
    total_width: u32,
    total_height: u32,
    samples_len: usize,
    sample_rate: u32,
    num_frames: usize,
) {
    let time_scale = Scale::uniform(24.0);
    let total_time = samples_len as f32 / sample_rate as f32;
    let num_time_ticks = (total_time / 5.0).ceil() as i32;

    for i in 0..=num_time_ticks {
        let time = i as f32 * 5.0;
        if time > total_time {
            break;
        }

        let x_pos = margin_left + ((time / total_time * num_frames as f32) as u32);
        let minutes = (time as i32) / 60;
        let seconds = (time as i32) % 60;

        // Prevent x_pos from exceeding total_width - margin_right
        if x_pos < (total_width - margin_right) {
            draw_text_mut(
                img,
                Rgb([0, 0, 0]),
                x_pos as i32 - 30,
                (total_height - margin_bottom) as i32 + 20,
                time_scale,
                font,
                &format!("{:01}:{:02}", minutes, seconds),
            );
            // Tick marks
            draw_line_segment_mut(
                img,
                (x_pos as f32, (total_height - margin_bottom) as f32),
                (x_pos as f32, (total_height - margin_bottom + 5) as f32),
                Rgb([0, 0, 0]),
            );
        }
    }
}

// Draw colorbar with scale
fn draw_colorbar_with_scale(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    font: &Font,
    margin_top: u32,
    colorbar_x: u32,
    colorbar_width: u32,
    colorbar_height: u32,
    gradient: &colorgrad::Gradient,
) {
    // Draw colorbar
    for y in 0..colorbar_height {
        let normalized = 1.0 - (y as f32 / colorbar_height as f32);
        let color = gradient.at(normalized as f64).to_rgba8();
        for x in 0..colorbar_width {
            img.put_pixel(
                colorbar_x + x,
                y + margin_top,
                Rgb([color[0], color[1], color[2]]),
            );
        }
    }

    // Draw colorbar border
    let border_color = Rgb([0, 0, 0]);
    // Left border
    draw_line_segment_mut(
        img,
        (colorbar_x as f32, margin_top as f32),
        (colorbar_x as f32, (margin_top + colorbar_height) as f32),
        border_color,
    );
    // Right border
    draw_line_segment_mut(
        img,
        ((colorbar_x + colorbar_width) as f32, margin_top as f32),
        (
            (colorbar_x + colorbar_width) as f32,
            (margin_top + colorbar_height) as f32,
        ),
        border_color,
    );
    // Top border
    draw_line_segment_mut(
        img,
        (colorbar_x as f32, margin_top as f32),
        ((colorbar_x + colorbar_width) as f32, margin_top as f32),
        border_color,
    );
    // Bottom border
    draw_line_segment_mut(
        img,
        (colorbar_x as f32, (margin_top + colorbar_height) as f32),
        (
            (colorbar_x + colorbar_width) as f32,
            (margin_top + colorbar_height) as f32,
        ),
        border_color,
    );

    // Calculate dB scale
    // Fixed dB scale range: -120dB to 0dB
    let db_min = -120.0;
    let db_max = 0.0;

    let db_start = db_min;
    let db_end = db_max;

    // Calculate dB values
    let mut db_values: Vec<f32> = Vec::new();
    let mut current_db = db_start;
    while current_db <= db_end {
        db_values.push(current_db);
        current_db += 10.0;
    }

    // Draw dB scale
    let db_scale = Scale::uniform(20.0);
    let denom = db_max - db_min;
    for &db_value in &db_values {
        // Convert dB value to normalized value
        let mut normalized = (db_value - db_min) / denom;
        if !normalized.is_finite() {
            normalized = 0.0;
        }
        normalized = normalized.max(0.0).min(1.0);
        let y_pos = margin_top + ((1.0 - normalized) * colorbar_height as f32) as u32;

        if y_pos >= margin_top && y_pos <= (margin_top + colorbar_height) {
            draw_text_mut(
                img,
                Rgb([0, 0, 0]),
                (colorbar_x + colorbar_width + 5) as i32,
                y_pos as i32 - 8,
                db_scale,
                font,
                &format!("{:.0}dB", db_value),
            );

            draw_line_segment_mut(
                img,
                ((colorbar_x + colorbar_width) as f32, y_pos as f32),
                ((colorbar_x + colorbar_width + 5) as f32, y_pos as f32),
                border_color,
            );
        }
    }
}

fn main() {
    println!("Program : {}", env!("CARGO_PKG_NAME"));
    println!("Version : {}", env!("CARGO_PKG_VERSION"));
    println!("Author  : {}", env!("CARGO_PKG_AUTHORS"));
    println!("Built   : {}", build_time::BUILD_TIME);
    println!("─────────────────────────────────────────────────");

    let args = Args::parse();

    let output_path = args.output.unwrap_or_else(|| {
        let input_path = std::path::Path::new(&args.input);
        let stem = input_path.file_stem().unwrap_or_default();
        format!("{}.png", stem.to_string_lossy())
    });

    let (samples, sample_rate) =
        read_audio_samples(&args.input).expect("Failed to read audio file");

    let fft_size = args.fft_size;
    let hop_size = args.hop_size.unwrap_or(fft_size / 2);

    println!("Generating spectrogram...");
    let spectrogram = generate_spectrogram(&samples, sample_rate, fft_size, hop_size);

    spectrogram.save(&output_path).unwrap();
    println!("Spectrogram saved to: {}", output_path);
}
