//! Startup prompt for a data folder that can't be used, e.g. an external drive that
//! isn't plugged in. Runs in the setup hook, on the main thread, before any window or
//! `AppState` exists, so it uses rfd's synchronous native dialogs.

use std::path::Path;

use rfd::{FileDialog, MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};

use crate::error::AppResult;
use crate::paths::{self, Paths, Unavailable};

const RETRY: &str = "Retry";
const CHOOSE: &str = "Choose another folder…";
const DEFAULT: &str = "Use default";

/// Resolves the data folder, asking the user what to do until one is usable.
/// "Choose another folder" and "Use default" are saved to the bootstrap file.
pub fn resolve_paths_interactively(config_dir: &Path) -> AppResult<Paths> {
    loop {
        let paths = Paths::load(config_dir);
        let problem = match paths.check() {
            Ok(()) => return Ok(paths),
            Err(e) => e,
        };
        log::warn!(
            "data folder {} unavailable: {problem}",
            paths.data_dir.display()
        );

        let choice = MessageDialog::new()
            .set_level(MessageLevel::Warning)
            .set_title("Lectern can't find your library")
            .set_description(message(&paths.data_dir, &problem))
            .set_buttons(MessageButtons::YesNoCancelCustom(
                RETRY.into(),
                CHOOSE.into(),
                DEFAULT.into(),
            ))
            .show();

        match choice {
            MessageDialogResult::Custom(c) if c == CHOOSE => {
                if let Some(dir) = FileDialog::new()
                    .set_title("Choose Lectern data folder")
                    .set_directory(paths::default_data_dir().parent().unwrap_or(Path::new("/")))
                    .pick_folder()
                {
                    paths.set_data_dir(&dir)?;
                }
            }
            MessageDialogResult::Custom(c) if c == DEFAULT => paths.clear_data_dir()?,
            // Retry, or the dialog was dismissed: never pick something on the user's behalf.
            _ => {}
        }
    }
}

fn message(data_dir: &Path, problem: &Unavailable) -> String {
    let what = match problem {
        Unavailable::MissingVolume { volume } => format!(
            "Your data folder is set to “{}”, but the drive “{}” isn't connected.",
            data_dir.display(),
            volume
                .file_name()
                .map(|n| n.to_string_lossy())
                .unwrap_or_default()
        ),
        Unavailable::NotWritable(e) => format!(
            "Your data folder “{}” can't be opened: {e}",
            data_dir.display()
        ),
    };
    format!(
        "{what}\n\nReconnect it and press Retry, or open Lectern from another folder \
         (the default is “{}”). If the drive is available again later, you can switch \
         back in Settings → Data folder.",
        paths::default_data_dir().display()
    )
}
