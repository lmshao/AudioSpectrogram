use clap::Parser;
use hound::{SampleFormat, WavReader};
use image::{ImageBuffer, Rgb};
use imageproc::drawing::{draw_line_segment_mut, draw_text_mut};
use rustfft::{FftPlanner, num_complex::Complex};
use rusttype::{Font, Scale};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Input WAV file path
    #[arg(short, long)]
    input: String,

    /// Output spectrogram file path
    #[arg(short, long, default_value = "spectrogram.png")]
    output: String,

    /// FFT size
    #[arg(short, long, default_value_t = 4096)]
    fft_size: usize,

    /// Hop size (defaults to half of FFT size)
    #[arg(short = 'p', long)]
    hop_size: Option<usize>,
}

fn main() {
    let args = Args::parse();

    let (samples, sample_rate) = read_wav_samples(&args.input).expect("Failed to read WAV file");

    let fft_size = args.fft_size;
    let hop_size = args.hop_size.unwrap_or(fft_size / 2);

    let spectrogram = generate_spectrogram(&samples, sample_rate, fft_size, hop_size);

    spectrogram.save(&args.output).unwrap();
    println!("Spectrogram saved to: {}", args.output);
}

fn read_wav_samples(path: &str) -> Result<(Vec<f32>, u32), hound::Error> {
    let reader = WavReader::open(path)?;
    let sample_rate = reader.spec().sample_rate;
    let sample_format = reader.spec().sample_format;
    println!("sample_format: {:?}", reader.spec());

    let samples: Vec<f32> = match sample_format {
        SampleFormat::Int => reader
            .into_samples::<i32>()
            .map(|s| s.unwrap() as f32 / i32::MAX as f32)
            .collect(),
        SampleFormat::Float => reader.into_samples::<f32>().map(|s| s.unwrap()).collect(),
    };

    Ok((samples, sample_rate))
}

fn compute_spectrum(samples: &[f32], fft_size: usize) -> Vec<f32> {
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(fft_size);

    // 1. Apply Hanning window and convert to complex input
    // Window function reduces spectral leakage
    let window: Vec<f32> = (0..fft_size)
        .map(|i| {
            0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (fft_size as f32 - 1.0)).cos())
        })
        .collect();
    let input: Vec<Complex<f32>> = samples
        .iter()
        .take(fft_size)
        .zip(window.iter())
        .map(|(s, w)| Complex::new(s * w, 0.0))
        .collect();

    // 2. Perform FFT
    let mut spectrum = input.clone();
    fft.process(&mut spectrum);

    // 3. Compute magnitude spectrum (take first half due to symmetry)
    spectrum[..fft_size / 2].iter().map(|c| c.norm()).collect()
}

fn generate_spectrogram(
    samples: &[f32],
    sample_rate: u32,
    fft_size: usize,
    hop_size: usize,
) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    // Set margins for scale drawing
    let margin_left = 120u32; // Left margin for frequency scale
    let margin_right = 120u32; // Right margin, symmetric with left
    let margin_top = 60u32; // Top margin, symmetric with bottom
    let margin_bottom = 60u32; // Bottom margin for time scale

    // Calculate main plotting area dimensions
    let num_frames = (samples.len() - fft_size) / hop_size + 1;
    let height = fft_size / 2;
    println!("num_frames: {}, height: {}", num_frames, height);

    // Create image with margins
    let total_width = (num_frames as u32) + margin_left + margin_right;
    let total_height = (height as u32) + margin_top + margin_bottom;
    let mut img = ImageBuffer::from_fn(total_width, total_height, |_, _| Rgb([255, 255, 255]));

    // Store all spectral values to calculate global min/max
    let mut all_magnitudes = Vec::new();
    let gradient = colorgrad::turbo();

    // First calculate all spectral values
    for i in 0..num_frames {
        let start = i * hop_size;
        let chunk = &samples[start..start + fft_size];
        let spectrum = compute_spectrum(chunk, fft_size);
        all_magnitudes.extend(spectrum);
    }

    println!("all_magnitudes len: {}", all_magnitudes.len());
    let max_magnitude = all_magnitudes.iter().fold(f32::MIN, |a, &b| a.max(b));
    let min_magnitude = all_magnitudes.iter().fold(f32::MAX, |a, &b| a.min(b));
    println!(
        "max_magnitude: {}, min_magnitude: {}",
        max_magnitude, min_magnitude
    );

    // Draw spectrogram body
    for (x, i) in (0..num_frames).enumerate() {
        let start = i * hop_size;
        let chunk = &samples[start..start + fft_size];
        let spectrum = compute_spectrum(chunk, fft_size);

        for (y, &magnitude) in spectrum.iter().enumerate() {
            let normalized = if max_magnitude > min_magnitude {
                (magnitude.log10() - min_magnitude.log10())
                    / (max_magnitude.log10() - min_magnitude.log10())
            } else {
                0.0
            };

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
    let font_data: &[u8] = include_bytes!("C:\\Windows\\Fonts\\consola.ttf");
    let font = Font::try_from_bytes(font_data).unwrap();

    // Draw axes
    let black = Rgb([0, 0, 0]);
    // Vertical axis (frequency)
    draw_line_segment_mut(
        &mut img,
        (margin_left as f32, 0.0),
        (margin_left as f32, height as f32),
        black,
    );
    // Horizontal axis (time)
    draw_line_segment_mut(
        &mut img,
        (margin_left as f32, height as f32),
        (total_width as f32, height as f32),
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

    // Draw border
    let border_color = Rgb([0, 0, 0]);
    // Left border
    draw_line_segment_mut(
        &mut img,
        (margin_left as f32, margin_top as f32),
        (margin_left as f32, (total_height - margin_bottom) as f32),
        border_color,
    );
    // Right border
    draw_line_segment_mut(
        &mut img,
        ((total_width - margin_right) as f32, margin_top as f32),
        (
            (total_width - margin_right) as f32,
            (total_height - margin_bottom) as f32,
        ),
        border_color,
    );
    // Top border
    draw_line_segment_mut(
        &mut img,
        (margin_left as f32, margin_top as f32),
        ((total_width - margin_right) as f32, margin_top as f32),
        border_color,
    );
    // Bottom border
    draw_line_segment_mut(
        &mut img,
        (margin_left as f32, (total_height - margin_bottom) as f32),
        (
            (total_width - margin_right) as f32,
            (total_height - margin_bottom) as f32,
        ),
        border_color,
    );

    // Draw colorbar legend on the right
    let colorbar_width = 30u32; // Colorbar width
    let colorbar_x = total_width - margin_right + 40; // Colorbar position
    let colorbar_height = height as u32; // Colorbar height same as spectrogram
    let db_scale = Scale::uniform(20.0); // Decibel scale font size
    let right_margin_extra = 100u32; // Extra right margin

    // Extend image width
    let new_width = total_width + right_margin_extra;
    let mut new_img = ImageBuffer::from_fn(new_width, total_height, |x, y| {
        if x < total_width {
            img.get_pixel(x, y).clone()
        } else {
            Rgb([255, 255, 255]) // Fill new area with white
        }
    });

    // Draw right decibel scale and colorbar
    draw_decibel_scale(
        &mut new_img,
        &font,
        margin_top,
        margin_bottom,
        colorbar_x,
        colorbar_width,
        colorbar_height,
        min_magnitude,
        max_magnitude,
        &gradient,
    );

    println!(
        "Generated spectrogram with dimensions: {}x{}",
        new_img.width(),
        new_img.height()
    );
    new_img
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
    let num_freq_ticks = (max_freq / 1000.0).ceil() as i32;

    // Draw frequency scale (including 0 to max frequency)
    for i in 0..=num_freq_ticks {
        let freq = if i == num_freq_ticks {
            max_freq // Ensure last tick is exact max frequency
        } else {
            i as f32 * 1000.0
        };

        let y_pos = if i == num_freq_ticks {
            margin_top // Highest frequency corresponds to top
        } else {
            total_height - margin_bottom - ((freq / max_freq * height_scale) as u32) - 1
        };

        if y_pos >= margin_top && y_pos < (total_height - margin_bottom) {
            // For max frequency, use actual frequency value
            let freq_text = if i == num_freq_ticks {
                format!("{:.1}kHz", max_freq / 1000.0)
            } else {
                format!("{:.1}kHz", freq / 1000.0)
            };

            draw_text_mut(
                img,
                Rgb([0, 0, 0]),
                5,
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
        }
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

// Draw right decibel scale and colorbar
fn draw_decibel_scale(
    img: &mut ImageBuffer<Rgb<u8>, Vec<u8>>,
    font: &Font,
    margin_top: u32,
    margin_bottom: u32,
    colorbar_x: u32,
    colorbar_width: u32,
    colorbar_height: u32,
    min_magnitude: f32,
    max_magnitude: f32,
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

    // Calculate decibel scale
    let db_min = min_magnitude.log10() * 20.0;
    let db_max = max_magnitude.log10() * 20.0;

    // Round down min value to nearest 10dB
    let db_start = (db_min / 10.0).floor() * 10.0;
    // Round up max value to nearest 10dB
    let db_end = (db_max / 10.0).ceil() * 10.0;

    // Calculate scale points with 10dB steps
    let mut db_values: Vec<f32> = Vec::new();
    let mut current_db = db_start;
    while current_db <= db_end {
        db_values.push(current_db);
        current_db += 10.0;
    }

    // Draw decibel scale
    let db_scale = Scale::uniform(20.0);
    for &db_value in &db_values {
        let normalized = (db_value - db_min) / (db_max - db_min);
        let y_pos = margin_top + ((1.0 - normalized) * colorbar_height as f32) as u32;

        if y_pos >= margin_top && y_pos <= (margin_top + colorbar_height) {
            draw_text_mut(
                img,
                Rgb([0, 0, 0]),
                (colorbar_x + colorbar_width + 10) as i32,
                y_pos as i32 - 10,
                db_scale,
                font,
                &format!("{:.0} dB", db_value),
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
