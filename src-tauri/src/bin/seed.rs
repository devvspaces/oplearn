//! Dev seed: imports a small public Creative Commons playlist into the data dir.
//! Usage: `pnpm seed` (override with LECTERN_SEED_PLAYLIST=<playlist id or URL>).

use lectern_lib::{db, paths::Paths, services::import::parse_youtube_url, services::ytdlp::YtDlp};

// MIT 6.824 Distributed Systems (Spring 2020), published by MIT under CC BY-NC-SA.
const DEFAULT_PLAYLIST: &str = "PLrw6a1wE39_tb2fErI4-WkMbsvGQk9_UB";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let input = std::env::var("LECTERN_SEED_PLAYLIST").unwrap_or_else(|_| DEFAULT_PLAYLIST.into());
    let playlist_id = parse_youtube_url(&input)
        .and_then(|p| p.playlist_id)
        .ok_or("not a playlist")?;
    let config_dir = dirs::config_dir()
        .ok_or("no config dir")?
        .join("com.lectern.app");
    let paths = Paths::resolve(&config_dir)?;
    let conn = db::open(&paths.db_file())?;
    let p = YtDlp::locate(&paths.bin_dir())?
        .dump_playlist_flat(&playlist_id)
        .await?;
    let (id, added) = db::repo::import::import_playlist(&conn, &p)?;
    println!(
        "seeded course {id} ({added} lessons) into {}",
        paths.db_file().display()
    );
    Ok(())
}
