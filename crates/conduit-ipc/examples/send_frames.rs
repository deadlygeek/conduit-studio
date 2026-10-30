//! Manual test driver for `conduit-preview`:
//!
//! ```sh
//! cargo run -p conduit-ipc --example send_frames | cargo run -p conduit-preview -- --save-dir /tmp/out
//! ```
//!
//! Sends a gradient with a semi-transparent stripe, then an error, then a second image,
//! then waits for 60s (closing stdout makes the preview exit).

use std::io::stdout;
use std::thread::sleep;
use std::time::Duration;

use conduit_ipc::{write_error, write_frame};

fn gradient(width: u32, height: u32, flip: bool) -> Vec<u8> {
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let (r, b) = ((x * 255 / width) as u8, (y * 255 / height) as u8);
            let alpha = if (height / 3..height / 2).contains(&y) {
                96
            } else {
                255
            };
            if flip {
                rgba.extend([b, 80, r, alpha]);
            } else {
                rgba.extend([r, 80, b, alpha]);
            }
        }
    }
    rgba
}

fn main() -> std::io::Result<()> {
    let mut out = stdout().lock();
    write_frame(&mut out, 800, 600, &gradient(800, 600, false))?;
    sleep(Duration::from_secs(3));
    write_error(
        &mut out,
        "comp.toml: input `base` refers to missing node `white_bgg`",
    )?;
    sleep(Duration::from_secs(3));
    write_frame(&mut out, 400, 300, &gradient(400, 300, true))?;
    sleep(Duration::from_secs(60));
    Ok(())
}
