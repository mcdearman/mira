//! A game run inside another program: the way an editor shows one.
//!
//! The host here opens the graphics device itself, as a program with its own interface
//! would, and owns the loop. It gives the device to the game (`App::host`), steps it
//! (`App::update`), and takes each frame as a texture (`render::frame_texture`) to show
//! however it likes. This host has no window: it runs the sacred-site game for a second,
//! changes the size of the space it gives it, and saves the last frame.
//!
//! ```sh
//! cargo run --example hosted_game -- frame.png
//! cargo run --example hosted_game -- frame.png examples/retro_dunk   # another game folder
//! cargo run --example hosted_game -- frame.png examples/retro_dunk 300   # 300 frames, of 1/30 s each
//! ```

use mira::{prelude::*, render::Screenshot};

/// The game that is hosted: the sacred sites, which are a folder of scripts.
const GAME: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/examples/sacred_sites");

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let saved = std::env::args().nth(1).unwrap_or("hosted.png".to_owned());
    let game = std::env::args().nth(2).unwrap_or(GAME.to_owned());
    // With a number of frames asked for, each is a thirtieth of a second of the game's time,
    // however fast they are run; else the game goes by the clock.
    let asked: Option<u32> = std::env::args().nth(3).and_then(|n| n.parse().ok());
    let frames = asked.unwrap_or(60);

    // The host's own device, opened with nothing asked of it.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default()))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default()))?;

    let mut app = App::new();
    app.add_plugins(DefaultPlugins);
    app.load_scripts(&game)?;
    if asked.is_some() {
        app.world.resource_mut::<Time>().set_fixed_step(Some(std::time::Duration::from_secs_f64(1.0 / 30.0)));
    }
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    app.host(device.clone(), queue, format, 640, 360);
    for frame in 0..frames {
        if frame == frames / 2 {
            app.host_resized(960, 540);
        }
        app.update();
        // A frame just after the size changed may not be drawn yet: there is then none to show.
        let Some(shown) = mira::render::frame_texture(&app.world) else {
            continue;
        };
        // What a host with an sRGB-encoded canvas samples: the bytes as stored.
        let _view = shown.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format.remove_srgb_suffix()),
            ..Default::default()
        });
        if frame == 0 || frame == frames - 1 {
            println!(
                "frame {frame}: {}x{} {:?}",
                shown.width(),
                shown.height(),
                shown.format()
            );
        }
    }
    app.world.resource_mut::<Screenshot>().request(&saved);
    app.update();
    // Reading the frame back takes the device a moment.
    app.update();
    println!("saved {saved}");
    Ok(())
}
