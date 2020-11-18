use anyhow::*;
use chrono::TimeZone;
use chrono::Utc;
use exif::{In, Tag};
use serde::*;
use std::fmt;
use std::fs::File;
use std::io::prelude::*;
use std::{ffi::OsStr, fs, io, path::Path, path::PathBuf};

// ---------------------------------------- Config ---------------------------------------- //

#[derive(Deserialize, Debug)]
struct Config {
    directories: ConfigDirectories,
}

#[derive(Deserialize, Debug)]
struct ConfigDirectories {
    unorganized: String,
    organized: String,
    duplicates: String,
}

fn load_config() -> io::Result<String> {
    let path = Path::new("./Photiso.toml");

    let mut file = File::open(&path)?;

    let mut s = String::new();
    file.read_to_string(&mut s)?;

    Ok(s)
}

// ---------------------------------------- exif helpers ---------------------------------------- //

// ---------------------------------------- PhotoInfo ---------------------------------------- //

#[derive(Debug)]
struct PhotoInfo {
    path: PathBuf,
    taken_date_time: chrono::DateTime<chrono::Utc>,
    length: u64, // created_date_time: Option<String>,
                 // modified_date_time: Option<String>,
                 // file_hash: Option<String>,
                 // length: Option<u64>
}

fn convert_exif_to_chrono_date_time(
    exif_date_time: &exif::DateTime,
) -> chrono::DateTime<chrono::Utc> {
    let date = chrono::Utc.ymd(
        exif_date_time.year as i32,
        exif_date_time.month as u32,
        exif_date_time.day as u32,
    );

    let date_time = date.and_hms_nano(
        exif_date_time.hour as u32,
        exif_date_time.minute as u32,
        exif_date_time.second as u32,
        exif_date_time.nanosecond.unwrap_or(0),
    );

    match exif_date_time.offset {
        Some(offset_minutes) => date_time + chrono::Duration::minutes(offset_minutes as i64),
        None => date_time,
    }
}

fn convert_exif_value_to_date_time(value: &exif::Value) -> Option<exif::DateTime> {
    if let exif::Value::Ascii(lines) = value {
        if lines.len() > 0 {
            if let Ok(date_time) = exif::DateTime::from_ascii(&lines[0]) {
                return Some(date_time);
            }
        }
    }

    None
}

fn convert_exif_value_to_u32(value: &exif::Value) -> Option<u32> {
    if let exif::Value::Ascii(lines) = value {
        if lines.len() > 0 {
            if let Ok(line) = std::str::from_utf8(&lines[0]) {
                if let Ok(number) = line.parse() {
                    return Some(number);
                }
            }
        }
    }

    None
}

fn convert_exif_value_to_string(value: &exif::Value) -> Option<String> {
    if let exif::Value::Ascii(lines) = value {
        if lines.len() > 0 {
            if let Ok(text) = std::str::from_utf8(&lines[0]) {
                return Some(text.to_string());
            }
        }
    }

    None
}

fn convert_system_time_to_chrono_date_time(
    value: &std::time::SystemTime,
) -> anyhow::Result<chrono::DateTime<Utc>> {
    let created_duration = value.duration_since(std::time::SystemTime::UNIX_EPOCH)?;
    Ok(chrono::Utc.timestamp(
        created_duration.as_secs() as i64,
        created_duration.subsec_nanos(),
    ))
}

fn get_exif_chrono_date_time(exif: &exif::Exif, tag: Tag) -> Option<chrono::DateTime<Utc>> {
    if let Some(field) = exif.get_field(tag, In::PRIMARY) {
        if let Some(exif_date_time) = convert_exif_value_to_date_time(&field.value) {
            let chrono_date_time = convert_exif_to_chrono_date_time(&exif_date_time);
            return Some(chrono_date_time);
        }
    }

    None
}

fn get_exif_field_u32(exif: &exif::Exif, tag: Tag) -> Option<u32> {
    if let Some(field) = exif.get_field(tag, In::PRIMARY) {
        return convert_exif_value_to_u32(&field.value);
    }

    None
}

fn get_exif_field_string(exif: &exif::Exif, tag: Tag) -> Option<String> {
    if let Some(field) = exif.get_field(tag, In::PRIMARY) {
        return convert_exif_value_to_string(&field.value);
    }

    None
}

fn get_exif_chrono_date_time_pair(
    exif: &exif::Exif,
    date_tag: Tag,
    sub_sec_tag: Tag,
) -> Option<chrono::DateTime<Utc>> {
    if let Some(date_time) = get_exif_chrono_date_time(exif, date_tag) {
        if let Some(sub_sec) = get_exif_field_u32(exif, sub_sec_tag) {
            return Some(date_time + chrono::Duration::milliseconds(sub_sec as i64));
        }

        return Some(date_time);
    }

    None
}

// DateTime, DateTimeOriginal, DateTimeDigitized
// OffsetTime, OffsetTimeOriginal, OffsetTimeDigitized
// SubSecTime, SubSecTimeOriginal, SubSecTimeDigitized
// GPSDateStamp,
fn get_photo_info(file_path: &Path) -> anyhow::Result<PhotoInfo> {
    let file = File::open(&file_path)?;

    println!("file: {:?}", file_path);

    let mut bufreader = std::io::BufReader::new(&file);
    let exifreader = exif::Reader::new();
    if let Ok(exif) = exifreader.read_from_container(&mut bufreader) {
        // exif original
        if let Some(date_time) =
            get_exif_chrono_date_time_pair(&exif, Tag::DateTime, Tag::SubSecTime)
        {
            println!("  date_time:           {:?}", date_time);
        }

        // exif original
        if let Some(date_time_original) =
            get_exif_chrono_date_time_pair(&exif, Tag::DateTimeOriginal, Tag::SubSecTimeOriginal)
        {
            println!("  date_time_original:  {:?}", date_time_original);
        }

        // exif digitized
        if let Some(date_time_digitized) =
            get_exif_chrono_date_time_pair(&exif, Tag::DateTimeDigitized, Tag::SubSecTimeDigitized)
        {
            println!("  date_time_digitized: {:?}", date_time_digitized);
        }
    }

    let metadata = fs::metadata(file_path)?;

    // file created
    let created_date_time = convert_system_time_to_chrono_date_time(&metadata.created()?)?;
    println!("  created_date_time:   {:?}", created_date_time);

    // file modified
    let modified_date_time = convert_system_time_to_chrono_date_time(&metadata.modified()?)?;
    println!("  modified_date_time:  {:?}", modified_date_time);

    let photo_info = PhotoInfo {
        path: PathBuf::from(&file_path),
        taken_date_time: Utc::now(),
        length: metadata.len(),
    };

    return Ok(photo_info);
}

fn print_all_exif(file_path: &Path) -> anyhow::Result<()> {
    let file = File::open(&file_path)?;
    let mut buf_reader = std::io::BufReader::new(&file);
    let exif_reader = exif::Reader::new();
    let exif = exif_reader.read_from_container(&mut buf_reader)?;
    for f in exif.fields() {
        println!(
            "{} {} {}",
            f.tag,
            f.ifd_num,
            f.display_value().with_unit(&exif)
        );
    }

    Ok(())
}

fn is_photo_file(path: &Path) -> bool {
    if path.exists() && path.is_file() {
        if let Some(ext) = path.extension() {
            if let Some(str_ext) = ext.to_str() {
                match str_ext.to_lowercase().as_str() {
                    "jpg" => return true,
                    "jpeg" => return true,
                    "tiff" => return true,
                    _ => return false,
                }
            }
        }
    }

    false
}

// ---------------------------------------- Directories ---------------------------------------- //

fn get_photo_destination_directory(
    photo_info: &PhotoInfo,
    config: &Config,
) -> anyhow::Result<PathBuf> {
    println!("{:?}", photo_info.taken_date_time);
    let file_path = PathBuf::from(&config.directories.unorganized);
    //file_path.push(photo_info.taken_date_time.year().to_string());
    println!("{:?}", file_path);
    return Ok(file_path);
}

fn organize_photo_file(file_path: &Path, config: &Config) -> anyhow::Result<()> {
    //print_all_exif(file_path)?;

    let photo_info = get_photo_info(file_path)?;
    //print!("{:?}", photo_info);
    //println!();

    //let dest_path = get_photo_destination_directory(&photo_info, config)?;
    // println!("{:?}", dest_path);

    Ok(())
}

fn organize_directory(dir_path: &Path, config: &Config) -> anyhow::Result<()> {
    // file entries immediately in this directory
    let mut entries = fs::read_dir(&dir_path)?
        .map(|res| res.map(|e| e.path()))
        .collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort();

    // photos in this directory
    for e in entries.iter().filter(|e| is_photo_file(&e)) {
        organize_photo_file(&e, config)?;
    }

    Ok(())
}

// ---------------------------------------- Main  ---------------------------------------- //

fn main() -> anyhow::Result<()> {
    println!("Hello Photiso");

    let config_text = load_config()?;
    let config: Config = toml::from_str(&config_text).unwrap();

    // print!("{:?}", config);
    // println!();

    let unorganized_dir = Path::new(&config.directories.unorganized);
    // println!("Unorganized: {:?}", unorganized_dir);
    organize_directory(&unorganized_dir, &config)?;

    // let  file_path = Path::new("./bikelane.jpg");
    // let photo_info = get_photo_info(&file_path)?;
    // print!("{:?}", photo_info);
    // println!();

    // println!("unorganized: {}", config.directories.unorganized);
    // println!("organized: {}", config.directories.organized);
    // println!("duplicates: {}", config.directories.duplicates);

    Ok(())
}

// fn list_some_files() -> io::Result<()> {
//     let mut entries = fs::read_dir("C:\\GitHub\\Photiso\\v2")?
//         .map(|res| res.map(|e| e.path()))
//         .collect::<Result<Vec<_>, io::Error>>()?;
//     entries.sort();

//     for entry in entries.into_iter(){
//         println!("{:?}", entry);
//     }
//     Ok(())
// }

// props: unorganizedDir, organizedDir, duplicatesDir,
// onShouldContinue, onStartedDir, onFinishedDir, onStartedFile, onFinishedFile
// onNoOp, onSkipped, onMoved, onDuplicateMoved, onError
// photo info: fileName, takenDateTime, createdDateTime, modifiedDateTime, fileHash, length, bestDateTime

// fn organize() {}
// fn organize_directory() {}
// fn organize_file() {}
// fn get_photo_info() {}
// fn move_photo() {}
// fn is_photo {}
// fn is_same_photo() {}
// fn get_photo_destination_directory() {}
// fn get_photo_destination_filename() {}
