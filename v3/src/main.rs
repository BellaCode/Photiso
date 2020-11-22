mod config;
mod file_hash;
mod photo_date_time;

use crate::config::*;
use crate::file_hash::*;
use crate::photo_date_time::*;

use std::{ffi::OsString, fs, io, path::Path, path::PathBuf};

enum PhotisoEvent<'a> {
    DirStarted {
        dir: &'a Path,
    },
    DirFinished {
        dir: &'a Path,
    },
    FileStarted {
        file: &'a Path,
    },
    FileFinished {
        file: &'a Path,
    },
    FileError {
        file: &'a Path,
        error: anyhow::Error,
    },
    Moved {
        from: &'a Path,
        to: &'a Path,
    },
    NoOp {
        file: &'a Path,
    },
    DuplicateMoved {
        from: &'a Path,
        to: &'a Path,
    },
    Skipped {
        file: &'a Path,
        reason: String,
    },
}

fn main() -> anyhow::Result<()> {
    println!("----------------------------------------");
    println!("Photiso");

    let config: Config = load_config()?;

    println!(
        "unorganized directory: {:?}",
        config.directories.unorganized
    );
    println!("organized directory: {:?}", config.directories.organized);
    println!("duplicates directory: {:?}", config.directories.duplicates);

    println!("----------------------------------------");

    organize_directory(config.directories.unorganized.as_ref(), &config)?;

    println!("----------------------------------------");

    Ok(())
}

fn organize_directory(dir_path: &Path, config: &Config) -> anyhow::Result<()> {
    // do not process the duplicates directory
    if dir_path == config.directories.duplicates {
        return Ok(());
    }

    on_photiso_event(PhotisoEvent::DirStarted { dir: dir_path }, config);

    let mut entries = fs::read_dir(&dir_path)?
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort();

    // organize files in this directory
    for e in entries.iter().filter(|e| e.is_file()) {
        organize_file(&e, config)?;
    }

    // organize child directories
    for e in entries.iter().filter(|e| e.is_dir()) {
        organize_directory(&e, config)?;
    }

    on_photiso_event(PhotisoEvent::DirFinished { dir: dir_path }, config);

    Ok(())
}

fn organize_file(file_path: &Path, config: &Config) -> anyhow::Result<()> {
    on_photiso_event(PhotisoEvent::FileStarted { file: file_path }, config);

    // only handle files with photo extensions
    if !is_photo_file(file_path) {
        on_photiso_event(
            PhotisoEvent::Skipped {
                file: file_path,
                reason: String::from("Not a photo."),
            },
            config,
        );
        return Ok(());
    }

    // never move a file with ! in the name
    if file_path
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .contains("!")
    {
        on_photiso_event(
            PhotisoEvent::Skipped {
                file: file_path,
                reason: String::from("Photo file name contains '!'."),
            },
            config,
        );
        return Ok(());
    }

    let photo_date_time_info = PhotoDateTimeInfo::load(file_path)?;
    let photo_date_time = photo_date_time_info.best();

    let mut conflict = 0;
    loop {
        let dest_path = get_photo_destination_path(file_path, &photo_date_time, conflict, config)?;

        // if the file is already in the right place, do nothing
        if file_path.to_str() == dest_path.to_str() {
            on_photiso_event(PhotisoEvent::NoOp { file: file_path }, config);
            break;
        }

        // if there is already a file in this location,
        if dest_path.exists() {
            match are_same_file_contents(file_path, &dest_path)? {
                Some(hash) => {
                    move_to_duplicates(file_path, &photo_date_time, &hash, config)?;
                    break;
                }
                None => {
                    // if there is a different file in this location, then try again with a higher conflict number
                    conflict += 1;
                    continue;
                }
            }
        } else {
            // move the file to the destination
            move_file(file_path, dest_path.as_ref())?;
            on_photiso_event(
                PhotisoEvent::Moved {
                    from: file_path,
                    to: &dest_path,
                },
                config,
            );
            break;
        }
    }

    on_photiso_event(PhotisoEvent::FileFinished { file: file_path }, config);
    Ok(())
}

fn move_to_duplicates(
    file_path: &Path,
    date_time: &chrono::DateTime<Utc>,
    hash: &str,
    config: &Config,
) -> anyhow::Result<()> {
    let mut conflict = 0;
    loop {
        let dest_path = get_photo_duplicates_path(file_path, date_time, hash, conflict, config)?;

        if file_path.to_str() == dest_path.to_str() {
            on_photiso_event(PhotisoEvent::NoOp { file: file_path }, config);
            break;
        }

        if dest_path.exists() {
            conflict += 1;
            continue;
        }

        move_file(file_path, dest_path.as_ref())?;
        on_photiso_event(
            PhotisoEvent::DuplicateMoved {
                from: file_path,
                to: &dest_path,
            },
            config,
        );
        break;
    }

    Ok(())
}

fn is_photo_file(path: &Path) -> bool {
    if path.exists() && path.is_file() {
        if let Some(ext) = path.extension() {
            if let Some(str_ext) = ext.to_str() {
                match str_ext.to_lowercase().as_str() {
                    "bmp" => return true,
                    "gif" => return true,
                    "jpg" => return true,
                    "jpeg" => return true,
                    "png" => return true,
                    "tif" => return true,
                    "tiff" => return true,
                    "wmp" => return true,
                    _ => return false,
                }
            }
        }
    }

    false
}

// returns the file hash if both files has the same length and hash values
fn are_same_file_contents(x: &Path, y: &Path) -> anyhow::Result<Option<String>> {
    let x_len = fs::metadata(x)?.len();
    let y_len = fs::metadata(y)?.len();

    if x_len != y_len {
        return Ok(None);
    }

    let x_hash = get_file_hash(x)?;
    let y_hash = get_file_hash(y)?;

    if x_hash != y_hash {
        return Ok(None);
    }

    return Ok(Some(x_hash));
}

fn get_photo_destination_path(
    file_path: &Path,
    date_time: &chrono::DateTime<Utc>,
    conflict: u32,
    config: &Config,
) -> anyhow::Result<PathBuf> {
    // folder path is '(organized)year/month' (i.e. YYYY/MM)
    let mut dest_path = PathBuf::from(&config.directories.organized);
    dest_path.push(PathBuf::from(date_time.format("%Y").to_string()).as_path());
    dest_path.push(PathBuf::from(date_time.format("%m").to_string()).as_path());

    // file name is 'year-month-day hour-minute-second-fractionOfSeconds conflict' (i.e. YYYY-MM-DD HH-MM-SS-FFFFFFFFF CCC)
    let mut file_name = date_time.format("%Y-%m-%d %H-%M-%S-%f").to_string();

    // add .jpg to the end so that set_extension doesn't overwrite the last part
    if conflict > 0 {
        file_name = format!("{} {:03}.jpg", file_name, conflict);
    } else {
        file_name = format!("{}.jpg", file_name);
    }

    dest_path.push(PathBuf::from(file_name).as_path());

    // extension is maintained, but made lowercase for consistency
    let extension = OsString::from(
        file_path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase(),
    );

    dest_path.set_extension(extension);

    return Ok(dest_path);
}

fn get_photo_duplicates_path(
    file_path: &Path,
    date_time: &chrono::DateTime<Utc>,
    hash: &str,
    conflict: u32,
    config: &Config,
) -> anyhow::Result<PathBuf> {
    // folder path is (duplicates)year/month (i.e. YYYY/MM)
    let mut dest_path = PathBuf::from(&config.directories.duplicates);
    dest_path.push(PathBuf::from(date_time.format("%Y").to_string()).as_path());
    dest_path.push(PathBuf::from(date_time.format("%m").to_string()).as_path());

    // file name is 'hash conflict'
    let mut file_name = format!("{}", hash);

    // add .jpg to the end so that set_extension doesn't overwrite the last part
    if conflict > 0 {
        file_name = format!("{}.{:03}.jpg", file_name, conflict);
    } else {
        file_name = format!("{}.jpg", file_name);
    }

    dest_path.push(PathBuf::from(file_name).as_path());

    // extension is maintained, but made lowercase for consistency
    let extension = OsString::from(
        file_path
            .extension()
            .unwrap()
            .to_str()
            .unwrap()
            .to_lowercase(),
    );
    dest_path.set_extension(extension);

    return Ok(dest_path);
}

fn move_file(from: &Path, to: &Path) -> io::Result<()> {
    let from_dir = to.parent().unwrap();
    fs::create_dir_all(from_dir)?;
    fs::rename(from, to)?;

    Ok(())
}

fn try_trim_unc_prefix(path: &Path) -> &Path {
    try_trim_prefix(path, r"\")
}

fn try_trim_prefix<P>(path: &Path, base: P) -> &Path
where
    P: AsRef<Path>,
{
    match path.strip_prefix(base) {
        Ok(new_path) => new_path.as_ref(),
        Err(e) => {
            println!("trim_prefix error. {:?}", e.to_string());
            return path.as_ref();
        }
    }
}

fn on_photiso_event(event: PhotisoEvent, config: &Config) {
    match event {
        PhotisoEvent::DirStarted { dir } => {
            let trim_dir = try_trim_prefix(dir, &config.directories.unorganized);
            println!(
                "Dir: (unorganized){}{:?}",
                std::path::MAIN_SEPARATOR,
                trim_dir
            );
        }
        PhotisoEvent::FileStarted { file } => {
            let trim_file = try_trim_prefix(file, &config.directories.unorganized);
            println!(
                "  File: (unorganized){}{:?}",
                std::path::MAIN_SEPARATOR,
                trim_file
            );
        }
        PhotisoEvent::Moved { from, to } => {
            let trim_from = try_trim_prefix(from, &config.directories.unorganized);
            let trim_to = try_trim_prefix(to, &config.directories.organized);
            println!(
                "    Moved: (unorganized){}{:?} -> (organized){}{:?}",
                std::path::MAIN_SEPARATOR,
                trim_from,
                std::path::MAIN_SEPARATOR,
                trim_to
            );
        }
        PhotisoEvent::DuplicateMoved { from, to } => {
            let trim_from = try_trim_prefix(from, &config.directories.unorganized);
            let trim_to = try_trim_prefix(to, &config.directories.duplicates);
            println!(
                "    Moved: (unorganized){}{:?} -> (duplicates){}{:?}",
                std::path::MAIN_SEPARATOR,
                trim_from,
                std::path::MAIN_SEPARATOR,
                trim_to
            );
        }
        PhotisoEvent::NoOp { file } => {
            let trim_file = try_trim_prefix(file, &config.directories.unorganized);
            println!(
                "    No-op: (organized){}{:?}",
                std::path::MAIN_SEPARATOR,
                trim_file
            );
        }
        PhotisoEvent::Skipped { file, reason } => {
            let trim_file = try_trim_prefix(file, &config.directories.unorganized);
            println!(
                "    Skipped - {}: (unorganized){}{:?}",
                reason,
                std::path::MAIN_SEPARATOR,
                trim_file
            );
        }

        _ => {}
    }
}
