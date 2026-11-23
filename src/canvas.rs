//! The paint job format, shared by the game and the server: which colours
//! there are, and how a paint job is packed to be sent. Drawing paint jobs on
//! the player model is in `paint.rs`.

/// Texels across (and down) a paint job.
pub const PAINT_SIZE: usize = 128;
pub const TEXELS: usize = PAINT_SIZE * PAINT_SIZE;

/// The colours paint comes in. The first eight are the colours the server
/// hands out, so a plain coat in colour `n` is palette colour `n`.
pub const PALETTE: [[u8; 3]; 32] = [
    [242, 64, 64],
    [64, 153, 255],
    [77, 230, 89],
    [255, 204, 38],
    [217, 89, 242],
    [38, 230, 217],
    [255, 128, 26],
    [242, 242, 242],
    // Greys.
    [18, 18, 21],
    [62, 64, 72],
    [118, 121, 130],
    [184, 187, 194],
    // Reds and pinks.
    [122, 22, 32],
    [196, 40, 60],
    [255, 112, 164],
    [255, 192, 212],
    // Skin, browns and sand.
    [255, 218, 178],
    [228, 172, 112],
    [178, 116, 64],
    [96, 58, 32],
    [205, 180, 128],
    [255, 236, 150],
    // Greens.
    [26, 82, 40],
    [112, 132, 58],
    [176, 232, 72],
    [92, 108, 90],
    // Blues and purples.
    [18, 36, 104],
    [110, 196, 255],
    [0, 116, 120],
    [72, 34, 128],
    [164, 116, 232],
    [200, 150, 30],
];

/// Bumped whenever the encoding changes.
const PAINT_FORMAT: u8 = 1;
/// No encoded paint job is longer than this.
pub const MAX_PAINT_BYTES: usize = 1 + 2 * TEXELS;

/// One palette index per texel, row by row from the top.
#[derive(Clone, PartialEq, Debug)]
pub struct Canvas(pub Vec<u8>);

impl Canvas {
    pub fn solid(color: u8) -> Canvas {
        Canvas(vec![color % PALETTE.len() as u8; TEXELS])
    }

    /// Runs of one colour, as (length - 1, colour) pairs after a format byte.
    /// A paint job is mostly large patches, so this is usually a few KB.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![PAINT_FORMAT];
        let mut i = 0;
        while i < self.0.len() {
            let color = self.0[i];
            let mut run = 1;
            while run < 256 && i + run < self.0.len() && self.0[i + run] == color {
                run += 1;
            }
            out.push((run - 1) as u8);
            out.push(color);
            i += run;
        }
        out
    }

    /// Reads what `encode` wrote. Anything else (wrong size, colours that do
    /// not exist) is refused.
    pub fn decode(bytes: &[u8]) -> Option<Canvas> {
        let (&format, runs) = bytes.split_first()?;
        if format != PAINT_FORMAT || runs.len() % 2 != 0 || bytes.len() > MAX_PAINT_BYTES {
            return None;
        }
        let mut pixels = Vec::with_capacity(TEXELS);
        for pair in runs.chunks_exact(2) {
            let (run, color) = (pair[0] as usize + 1, pair[1]);
            if color as usize >= PALETTE.len() || pixels.len() + run > TEXELS {
                return None;
            }
            pixels.extend(std::iter::repeat_n(color, run));
        }
        (pixels.len() == TEXELS).then_some(Canvas(pixels))
    }

    /// The colour used the most: the dot beside their name in the player list.
    pub fn main_color(&self) -> u8 {
        let mut counts = [0usize; PALETTE.len()];
        for &color in &self.0 {
            counts[color as usize % PALETTE.len()] += 1;
        }
        (0..PALETTE.len()).max_by_key(|i| counts[*i]).unwrap_or(0) as u8
    }
}
