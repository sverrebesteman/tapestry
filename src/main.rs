use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use colored::Colorize;
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgb, RgbImage};
use std::collections::HashMap;
use std::path::PathBuf;

// ─── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(name = "tapestry", about = "tapestry crochet pattern tool 🧶")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// convert image to crochet chart
    Chart {
        image: PathBuf,
        #[arg(short, long, value_name = "HEX", num_args = 1..)]
        palette: Vec<String>,
        #[arg(short, long, default_value = "60")]
        width: u32,
        #[arg(long, default_value = "0")]
        height: u32,
        #[arg(short, long, default_value = "blocks")]
        mode: DisplayMode,
        #[arg(short, long)]
        dither: bool,
        #[arg(short, long)]
        instructions: bool,
        #[arg(short, long)]
        counts: bool,
        #[arg(short, long, value_name = "FILE")]
        save: Option<PathBuf>,
        #[arg(long, default_value = "x")]
        symbol: String,
        /// draw separators between every stitch
        #[arg(short = 'g', long)]
        grid: bool,
        /// show column/row numbers like a chessboard
        #[arg(short = 'r', long)]
        ruler: bool,
        /// interactive row-by-row tracker (press enter to mark rows done)
        #[arg(long)]
        track: bool,
    },
    Generate {
        #[arg(short = 'P', long, default_value = "checkerboard")]
        pattern: PatternType,
        #[arg(short = 'p', long, value_name = "HEX", num_args = 1..)]
        palette: Vec<String>,
        #[arg(short = 'w', long, default_value = "40")]
        width: u32,
        #[arg(long, default_value = "20")]
        height: u32,
        #[arg(short = 't', long, default_value = "4")]
        tile: u32,
        #[arg(short = 'm', long, default_value = "blocks")]
        mode: DisplayMode,
        #[arg(short = 'i', long)]
        instructions: bool,
        #[arg(short = 's', long, value_name = "FILE")]
        save: Option<PathBuf>,
        /// draw separators between every stitch
        #[arg(short = 'g', long)]
        grid: bool,
        /// show column/row numbers like a chessboard
        #[arg(short = 'r', long)]
        ruler: bool,
        /// interactive row-by-row tracker (press enter to mark rows done)
        #[arg(long)]
        track: bool,
    },
    Preview {
        #[arg(num_args = 1..)]
        colors: Vec<String>,
    },
    /// estimate yarn usage
    Estimate {
        stitches: String,
        #[arg(short, long, default_value = "sc")]
        stitch: StitchType,
        #[arg(short, long, default_value = "worsted")]
        weight: YarnWeight,
    },
}

#[derive(ValueEnum, Clone, Debug)]
enum DisplayMode { Blocks, Ascii, Symbols, Hex }

#[derive(ValueEnum, Clone, Debug)]
enum PatternType { Checkerboard, Stripes, Diamonds, Chevron, Spiral, Noise }

#[derive(ValueEnum, Clone, Debug)]
enum StitchType { Sc, Hdc, Dc, Slst }

#[derive(ValueEnum, Clone, Debug)]
enum YarnWeight { Lace, Fingering, Sport, Dk, Worsted, Bulky, SuperBulky }

// ─── COLOR ───────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct Yarn { r: u8, g: u8, b: u8, hex: String }

impl Yarn {
    fn from_hex(hex: &str) -> Result<Self> {
        let h = hex.trim_start_matches('#');
        if h.len() != 6 { bail!("hex must be 6 chars: {}", hex); }
        let r = u8::from_str_radix(&h[0..2], 16)?;
        let g = u8::from_str_radix(&h[2..4], 16)?;
        let b = u8::from_str_radix(&h[4..6], 16)?;
        Ok(Self { r, g, b, hex: format!("#{}", h.to_uppercase()) })
    }
    fn dist(&self, r: u8, g: u8, b: u8) -> f32 {
        let dr = self.r as f32 - r as f32;
        let dg = self.g as f32 - g as f32;
        let db = self.b as f32 - b as f32;
        (0.30 * dr * dr + 0.59 * dg * dg + 0.11 * db * db).sqrt()
    }
}

fn closest(palette: &[Yarn], r: u8, g: u8, b: u8) -> usize {
    palette.iter().enumerate()
        .min_by(|(_, a), (_, c)| a.dist(r,g,b).partial_cmp(&c.dist(r,g,b)).unwrap())
        .map(|(i,_)| i).unwrap_or(0)
}

fn default_palette() -> Vec<Yarn> {
    ["#F5F5DC","#2C2C2C","#C0392B","#2980B9","#27AE60","#F39C12"]
        .iter().map(|h| Yarn::from_hex(h).unwrap()).collect()
}

fn parse_palette(hexes: &[String]) -> Result<Vec<Yarn>> {
    if hexes.is_empty() {
        eprintln!("no palette — using default tapestry palette");
        return Ok(default_palette());
    }
    hexes.iter().map(|h| Yarn::from_hex(h)).collect()
}

// ─── IMAGE → CHART ───────────────────────────────────────────────────────────

fn resize(img: &DynamicImage, w: u32, h: u32) -> RgbImage {
    let (iw, ih) = img.dimensions();
    let target_h = if h == 0 { ((w as f32 / iw as f32) * ih as f32 * 0.8) as u32 } else { h };
    img.resize_exact(w, target_h.max(1), image::imageops::FilterType::Lanczos3).to_rgb8()
}

fn quantize(img: &RgbImage, palette: &[Yarn], dither: bool) -> Vec<Vec<usize>> {
    let (w, h) = img.dimensions();
    let mut err: Vec<Vec<(f32,f32,f32)>> = vec![vec![(0.,0.,0.); w as usize]; h as usize];
    (0..h).map(|y| {
        (0..w).map(|x| {
            let px = img.get_pixel(x, y);
            let (er, eg, eb) = err[y as usize][x as usize];
            let r = (px[0] as f32 + er).clamp(0., 255.) as u8;
            let g = (px[1] as f32 + eg).clamp(0., 255.) as u8;
            let b = (px[2] as f32 + eb).clamp(0., 255.) as u8;
            let idx = closest(palette, r, g, b);
            if dither {
                let c = &palette[idx];
                let (qr, qg, qb) = (r as f32 - c.r as f32, g as f32 - c.g as f32, b as f32 - c.b as f32);
                let mut add = |dx: i32, dy: i32, f: f32| {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx >= 0 && nx < w as i32 && ny >= 0 && ny < h as i32 {
                        err[ny as usize][nx as usize].0 += qr * f;
                        err[ny as usize][nx as usize].1 += qg * f;
                        err[ny as usize][nx as usize].2 += qb * f;
                    }
                };
                add(1, 0, 7./16.); add(-1, 1, 3./16.);
                add(0, 1, 5./16.); add(1, 1, 1./16.);
            }
            idx
        }).collect()
    }).collect()
}

// ─── DISPLAY ─────────────────────────────────────────────────────────────────

struct GridOpts {
    grid:  bool,
    ruler: bool,
    track: bool,
}

fn stitch_cell(c: &Yarn, mode: &DisplayMode, sym: &str) -> String {
    match mode {
        DisplayMode::Blocks  => format!("{}", "██".truecolor(c.r, c.g, c.b)),
        DisplayMode::Ascii   => {
            let lum = 0.299*c.r as f32 + 0.587*c.g as f32 + 0.114*c.b as f32;
            (if lum > 200. { "░░" } else if lum > 100. { "▒▒" } else { "▓▓" }).to_string()
        }
        DisplayMode::Symbols => format!("{}", format!("{:2}", sym).truecolor(c.r, c.g, c.b)),
        DisplayMode::Hex     => format!("{:02x}", c.r / 16),
    }
}

fn print_chart(chart: &[Vec<usize>], palette: &[Yarn], mode: &DisplayMode, symbol: &str, g: &GridOpts) {
    let sym  = if symbol.is_empty() { "x" } else { &symbol[..symbol.len().min(2)] };
    let cols = chart.first().map(|r| r.len()).unwrap_or(0);
    let rows = chart.len();

    let row_num_w = format!("{}", rows).len();
    let left_gutter  = if g.ruler { row_num_w + 1 } else { 0 }; // "42 "
    // done-marker gutter (✓ or space)
    let done_w = if g.track { 3 } else { 0 }; // " ✓ "

    let vsep  = "|".truecolor(80, 80, 80).to_string();
    let hsep  = {
        let gutter = " ".repeat(left_gutter + done_w);
        let inner  = (0..cols).map(|_| "--").collect::<Vec<_>>().join("+");
        format!("{}+{}+", gutter, inner)
    };

    // ── column ruler ──────────────────────────────────────────────────────────
    // print numbers right-aligned into their cell, space-separated
    // each cell is 2 terminal cols; number ≥10 needs 2 chars, fits exactly
    // number <10 gets a leading space: " 1"
    let col_ruler = || {
        let gutter = " ".repeat(left_gutter + done_w + if g.grid { 1 } else { 0 });
        print!("{}", gutter);
        for x in 0..cols {
            print!("{}", format!("{:>2}", x + 1).dimmed());
            if g.grid && x + 1 < cols { print!(" "); } // grid pipe becomes space in ruler
        }
        println!();
    };

    if g.ruler {
        col_ruler();
        if g.grid { println!("{}", hsep); }
    }

    // ── rows ──────────────────────────────────────────────────────────────────
    let mut done: Vec<bool> = vec![false; rows];

    for (ri, row) in chart.iter().enumerate() {
        if g.track {
            // show tick if done, blank if not
            if done[ri] {
                print!("{} ", "✓".green().bold());
            } else {
                print!("  ");
            }
        }

        if g.ruler {
            print!("{:>width$} ", ri + 1, width = row_num_w);
        }
        if g.grid { print!("{}", vsep); }

        for (_, &idx) in row.iter().enumerate() {
            print!("{}", stitch_cell(&palette[idx], mode, sym));
            if g.grid {
                print!("{}", vsep);
            }
        }

        if g.ruler {
            print!(" {:<width$}", ri + 1, width = row_num_w);
        }

        if g.track {
            if done[ri] {
                print!("  {}", "✓".green().bold());
            } else {
                print!("  {}", "○".dimmed());
            }
        }
        println!();

        if g.grid && ri + 1 < rows { println!("{}", hsep); }
    }

    if g.grid { println!("{}", hsep); }
    if g.ruler { col_ruler(); }

    // ── interactive tracker ───────────────────────────────────────────────────
    if g.track {
        use std::io::{BufRead, Write};
        println!();
        println!("{}", "── row tracker ── enter row number to toggle done, or q to quit".dimmed());
        let stdin  = std::io::stdin();
        let stdout = std::io::stdout();
        loop {
            print!("row > ");
            stdout.lock().flush().ok();
            let mut line = String::new();
            stdin.lock().read_line(&mut line).ok();
            let input = line.trim();
            if input == "q" || input == "quit" { break; }
            if input == "all" {
                done.iter_mut().for_each(|d| *d = true);
            } else if input == "reset" {
                done.iter_mut().for_each(|d| *d = false);
            } else if let Ok(n) = input.parse::<usize>() {
                if n >= 1 && n <= rows {
                    done[n - 1] = !done[n - 1];
                } else {
                    println!("row {} out of range (1–{})", n, rows);
                    continue;
                }
            } else if !input.is_empty() {
                println!("enter a row number, 'all', 'reset', or 'q'");
                continue;
            }

            // redraw
            println!();
            for (ri, row) in chart.iter().enumerate() {
                let tick = if done[ri] { "✓".green().bold().to_string() } else { "○".dimmed().to_string() };
                if g.ruler {
                    print!("{:>width$} ", ri + 1, width = row_num_w);
                }
                print!("{} ", tick);
                if g.grid { print!("{}", vsep); }
                for (_, &idx) in row.iter().enumerate() {
                    print!("{}", stitch_cell(&palette[idx], mode, sym));
                    if g.grid { print!("{}", vsep); }
                }
                if g.ruler { print!(" {:<width$}", ri + 1, width = row_num_w); }
                println!();
                if g.grid && ri + 1 < rows { println!("{}", hsep); }
            }
            let done_count = done.iter().filter(|&&d| d).count();
            println!("{}/{} rows done", done_count, rows);
        }
    }
}

fn print_instructions(chart: &[Vec<usize>], palette: &[Yarn]) {
    println!("\n{}", "═══ ROW INSTRUCTIONS ═══".bold());
    println!("(read right → left as in tapestry crochet)\n");
    for (ri, row) in chart.iter().enumerate() {
        let mut runs: Vec<(usize, usize)> = Vec::new();
        let (mut cur, mut cnt) = (row[0], 1usize);
        for &idx in &row[1..] {
            if idx == cur { cnt += 1; } else { runs.push((cur, cnt)); cur = idx; cnt = 1; }
        }
        runs.push((cur, cnt));
        print!("row {:>3}:  ", ri + 1);
        for (ci, n) in &runs {
            let c = &palette[*ci];
            print!("{}  ", format!("{} ×{}", c.hex, n).truecolor(c.r, c.g, c.b));
        }
        println!();
    }
}

fn print_counts(chart: &[Vec<usize>], palette: &[Yarn]) {
    let mut counts: HashMap<usize, usize> = HashMap::new();
    let total: usize = chart.iter().map(|r| r.len()).sum();
    for row in chart { for &idx in row { *counts.entry(idx).or_insert(0) += 1; } }
    println!("\n{}", "═══ STITCH COUNTS ═══".bold());
    let mut entries: Vec<_> = counts.iter().collect();
    entries.sort_by_key(|(i, _)| *i);
    for (idx, count) in entries {
        let c = &palette[*idx];
        let pct = *count as f32 / total as f32 * 100.;
        let bar = "█".repeat((pct / 2.) as usize);
        println!("  {}  {:>6} stitches  ({:5.1}%)  {}",
            format!("{}", c.hex).truecolor(c.r, c.g, c.b).bold(),
            count, pct, bar.truecolor(c.r, c.g, c.b));
    }
    println!("  total: {} stitches", total);
}

fn save_png(chart: &[Vec<usize>], palette: &[Yarn], path: &PathBuf, scale: u32) -> Result<()> {
    let (h, w) = (chart.len() as u32, chart[0].len() as u32);
    let mut img: RgbImage = ImageBuffer::new(w * scale, h * scale);
    for (y, row) in chart.iter().enumerate() {
        for (x, &idx) in row.iter().enumerate() {
            let c = &palette[idx];
            for dy in 0..scale { for dx in 0..scale {
                img.put_pixel(x as u32 * scale + dx, y as u32 * scale + dy, Rgb([c.r, c.g, c.b]));
            }}
        }
    }
    img.save(path).with_context(|| format!("saving {}", path.display()))?;
    println!("saved → {}", path.display());
    Ok(())
}

// ─── GENERATE ────────────────────────────────────────────────────────────────

fn gen_pattern(kind: &PatternType, w: u32, h: u32, palette: &[Yarn], tile: u32) -> Vec<Vec<usize>> {
    let n = palette.len().max(1) as u32;
    (0..h).map(|y| (0..w).map(|x| {
        (match kind {
            PatternType::Checkerboard => (x / tile + y / tile) % n,
            PatternType::Stripes      => (y / tile) % n,
            PatternType::Diamonds     => {
                let dx = (x as i32 - w as i32 / 2).unsigned_abs();
                let dy = (y as i32 - h as i32 / 2).unsigned_abs();
                (dx + dy) / tile.max(1) % n
            }
            PatternType::Chevron => (x + y % (2 * tile.max(1))) % (2 * tile.max(1)) / (2 * tile.max(1) / n).max(1) % n,
            PatternType::Spiral => {
                let (cx, cy) = (w as f32 / 2., h as f32 / 2.);
                let angle = (y as f32 - cy).atan2(x as f32 - cx);
                let dist  = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
                ((angle / std::f32::consts::TAU * tile as f32 + dist / tile as f32) as u32) % n
            }
            PatternType::Noise =>
                (x.wrapping_mul(2654435761) ^ y.wrapping_mul(2246822519)) % n,
        }) as usize
    }).collect()).collect()
}

// ─── ESTIMATE ────────────────────────────────────────────────────────────────

fn estimate(stitches: &str, stitch: &StitchType, weight: &YarnWeight) -> Result<()> {
    let total: u64 = if stitches.contains(['x','×']) {
        let p: Vec<&str> = stitches.split(['x','×']).collect();
        if p.len() != 2 { bail!("use WxH, e.g. 60x40"); }
        p[0].trim().parse::<u64>()? * p[1].trim().parse::<u64>()?
    } else { stitches.parse()? };

    let cm = total as f32 * match stitch {
        StitchType::Sc => 1.8, StitchType::Hdc => 2.2,
        StitchType::Dc => 2.8, StitchType::Slst => 1.2,
    };
    let m_per_100g: f32 = match weight {
        YarnWeight::Lace => 800., YarnWeight::Fingering => 380., YarnWeight::Sport => 275.,
        YarnWeight::Dk => 220., YarnWeight::Worsted => 180.,
        YarnWeight::Bulky => 110., YarnWeight::SuperBulky => 60.,
    };
    let m = cm / 100.; let g = m / m_per_100g * 100.;
    println!("\n{}", "═══ YARN ESTIMATE ═══".bold());
    println!("  stitches:       {}", total);
    println!("  plain crochet:  ~{:.0} m  (~{:.0} g)", m, g);
    println!("  tapestry carry: ~{:.0} m  (~{:.0} g)  (×1.9 for carried yarn)", m*1.9, g*1.9);
    println!("  {}", "⚠  tapestry carries unused colour inside every stitch — yarn use ~doubles".yellow());
    Ok(())
}

// ─── MAIN ────────────────────────────────────────────────────────────────────

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.cmd {
        Cmd::Chart { image, palette, width, height, mode, dither,
                     instructions, counts, save, symbol, grid, ruler, track } => {
            let pal = parse_palette(palette)?;
            let img = image::open(image)
                .with_context(|| format!("opening {}", image.display()))?;
            println!("loaded {}×{} → resizing to {} stitches wide", img.width(), img.height(), width);
            let res = resize(&img, *width, *height);
            println!("chart size: {}×{} stitches", res.width(), res.height());
            let chart = quantize(&res, &pal, *dither);
            println!("\n{}", "═══ CHART ═══".bold());
            print_chart(&chart, &pal, mode, symbol, &GridOpts { grid: *grid, ruler: *ruler, track: *track });
            if *instructions { print_instructions(&chart, &pal); }
            if *counts        { print_counts(&chart, &pal); }
            if let Some(p) = save { save_png(&chart, &pal, p, 8)?; }
        }
        Cmd::Generate { pattern, palette, width, height, tile, mode, instructions, save, grid, ruler, track } => {
            let pal = parse_palette(palette)?;
            let chart = gen_pattern(pattern, *width, *height, &pal, *tile);
            println!("\n{}", "═══ CHART ═══".bold());
            print_chart(&chart, &pal, mode, "x", &GridOpts { grid: *grid, ruler: *ruler, track: *track });
            if *instructions { print_instructions(&chart, &pal); }
            if let Some(p) = save { save_png(&chart, &pal, p, 8)?; }
        }
        Cmd::Preview { colors } => {
            if colors.is_empty() {
                println!("pass hex codes:  tapestry preview #ff0000 #00ff88");
                return Ok(());
            }
            println!("\n{}", "═══ COLOUR PREVIEW ═══".bold());
            for hex in colors {
                let c = Yarn::from_hex(hex)?;
                let lum = 0.299*c.r as f32 + 0.587*c.g as f32 + 0.114*c.b as f32;
                println!("  {}  {}  lum:{:.0}",
                    "████████████".truecolor(c.r, c.g, c.b),
                    format!("{} r:{} g:{} b:{}", c.hex, c.r, c.g, c.b).truecolor(c.r, c.g, c.b).bold(),
                    lum);
            }
        }
        Cmd::Estimate { stitches, stitch, weight } => estimate(stitches, stitch, weight)?,
    }
    Ok(())
}
