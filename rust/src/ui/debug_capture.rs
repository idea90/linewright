//! A development aid, in debug builds only: LINEWRIGHT_CAPTURE=out.png saves a screenshot of the window once the
//! drawing has opened, then closes it. LINEWRIGHT_INPUT types command lines first, separated by semicolons; PROPERTIES
//! opens the Properties palette, ESC cancels, HOVER rests the pointer mid-drawing, MENU and SHIFTMENU right-click
//! there (SHIFTMENU with Shift held) and TYPE text leaves text in the command line. It lets the window be checked where no screenshot tool is at hand.

use eframe::egui;

pub struct Capture {
    path: String,
    inputs: Vec<String>,
    frame: usize,
    requested: bool,
}

impl Capture {
    pub fn from_env() -> Option<Capture> {
        let inputs = std::env::var("LINEWRIGHT_INPUT").map(|s| s.split(';').map(str::to_string).collect()).unwrap_or_default();
        Some(Capture { path: std::env::var("LINEWRIGHT_CAPTURE").ok()?, inputs, frame: 0, requested: false })
    }

    /// The next command line to type, a few frames apart once the drawing is shown.
    pub fn next_input(&mut self) -> Option<String> {
        (self.frame > 20 && self.frame.is_multiple_of(4) && !self.inputs.is_empty()).then(|| self.inputs.remove(0))
    }

    /// Call once a frame, when nothing is still loading.
    pub fn step(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        ctx.request_repaint();
        if self.frame > 40 && self.inputs.is_empty() && !self.requested {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = shot {
            std::fs::write(&self.path, png(image.width() as u32, image.height() as u32, image.as_raw())).expect("write screenshot");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { 0xedb8_8320 ^ (crc >> 1) } else { crc >> 1 };
        }
    }
    !crc
}

/// An RGBA image as a PNG, its pixels in uncompressed deflate blocks.
fn png(width: u32, height: u32, rgba: &[u8]) -> Vec<u8> {
    let chunk = |out: &mut Vec<u8>, kind: &[u8], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc32(&body).to_be_bytes());
    };
    let mut raw = vec![];
    for row in rgba.chunks(width as usize * 4) {
        raw.push(0);
        raw.extend(row);
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
    for (index, block) in blocks.iter().enumerate() {
        zlib.push((index + 1 == blocks.len()) as u8);
        zlib.extend((block.len() as u16).to_le_bytes());
        zlib.extend((!(block.len() as u16)).to_le_bytes());
        zlib.extend(*block);
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &raw {
        a = (a + byte as u32) % 65521;
        b = (b + a) % 65521;
    }
    zlib.extend(((b << 16) | a).to_be_bytes());
    let mut header = vec![];
    header.extend(width.to_be_bytes());
    header.extend(height.to_be_bytes());
    header.extend([8, 6, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut out, b"IHDR", &header);
    chunk(&mut out, b"IDAT", &zlib);
    chunk(&mut out, b"IEND", &[]);
    out
}
