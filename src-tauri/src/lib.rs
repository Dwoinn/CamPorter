// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use sysinfo::Disks;
use serde::Serialize;
use std::process::Command;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;
use tauri::{Emitter, Manager};
use base64::{engine::general_purpose, Engine as _};
use image::{ImageFormat};

#[cfg(not(target_os = "windows"))]
use thumbnails::Thumbnailer;

#[derive(Serialize)]
struct RemovableDrive {
    name: String,
    mount_point: String,
    device_id: String,
}

#[derive(Serialize)]
struct MediaFile {
    name: String,
    path: String,
    size: u64,
    modified: u64, // Unix timestamp
    extension: String,
    is_image: bool,
    is_video: bool,
}

#[tauri::command]
fn list_removable_drives() -> Vec<RemovableDrive> {
    let disks = Disks::new_with_refreshed_list();
    
    disks.iter()
        .filter(|disk| {
            let mount_point = disk.mount_point().to_string_lossy();
            let name = disk.name().to_string_lossy();
            
            // On Linux, removable drives often mount under /media, /mnt, or /run/media
            // Also check for common USB drive characteristics
            disk.is_removable() ||
            mount_point.starts_with("/media/") ||
            mount_point.starts_with("/mnt/") ||
            mount_point.starts_with("/run/media/") ||
            name.contains("sd") ||  // SD cards
            name.contains("usb") || // USB drives
            name.contains("removable")
        })
        .map(|disk| RemovableDrive {
            name: format!("{} ({})",
                disk.name().to_string_lossy(),
                if disk.is_removable() { "removable" } else { "mounted" }
            ),
            mount_point: disk.mount_point().to_string_lossy().to_string(),
            device_id: disk.name().to_string_lossy().to_string(),
        })
        .collect()
}

#[tauri::command]
fn list_media_files(drive_path: String) -> Result<Vec<MediaFile>, String> {
    let src = Path::new(&drive_path);
    
    if !src.exists() {
        return Err("Drive path does not exist".to_string());
    }
    
    let media_extensions = ["mp4", "jpg", "jpeg", "png", "mov", "heic", "mp3", "wav", "avi", "mkv", "gif"];
    let image_extensions = ["jpg", "jpeg", "png", "heic", "gif"];
    let video_extensions = ["mp4", "mov", "avi", "mkv"];
    
    let mut media_files = Vec::new();
    
    for entry in WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if media_extensions.contains(&ext_lower.as_str()) {
                    if let Ok(metadata) = entry.metadata() {
                        let modified = metadata
                            .modified()
                            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        
                        let file_name = entry.file_name().to_string_lossy().to_string();
                        let is_image = image_extensions.contains(&ext_lower.as_str());
                        let is_video = video_extensions.contains(&ext_lower.as_str());
                        
                        media_files.push(MediaFile {
                            name: file_name,
                            path: entry.path().to_string_lossy().to_string(),
                            size: metadata.len(),
                            modified,
                            extension: ext_lower,
                            is_image,
                            is_video,
                        });
                    }
                }
            }
        }
    }
    
    // Sort by modification time (newest first)
    media_files.sort_by(|a, b| b.modified.cmp(&a.modified));
    
    Ok(media_files)
}

#[tauri::command]
async fn unmount_drive(mount_point: String) -> Result<(), String> {
    let mut command = if cfg!(target_os = "linux") {
        let mut cmd = Command::new("udisksctl");
        cmd.args(["unmount", "-p", &mount_point]);
        cmd
    } else if cfg!(target_os = "macos") {
        let mut cmd = Command::new("diskutil");
        cmd.args(["unmount", &mount_point]);
        cmd
    } else if cfg!(target_os = "windows") {
        let mut cmd = Command::new("powershell");
        cmd.args([
            "-Command",
            &format!("(New-Object -comObject Shell.Application).Namespace(17).ParseName('{}').InvokeVerb('Eject')", mount_point),
        ]);
        cmd
    } else {
        return Err("Unsupported platform".to_string());
    };

    let output = command.output().map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(format!("Unmount failed: {} {}", stderr, stdout))
    }
}

#[tauri::command]
async fn import_selected_files(
    file_paths: Vec<String>,
    target_path: String,
    window: tauri::Window,
) -> Result<(), String> {
    let dest = Path::new(&target_path);
    
    // Create target directory if it doesn't exist
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    
    // Calculate total size of all files
    let mut total_size: u64 = 0;
    let mut file_sizes: Vec<u64> = Vec::new();
    
    for file_path in &file_paths {
        let src_file = Path::new(file_path);
        if src_file.exists() {
            if let Ok(metadata) = src_file.metadata() {
                let size = metadata.len();
                total_size += size;
                file_sizes.push(size);
            } else {
                file_sizes.push(0);
            }
        } else {
            file_sizes.push(0);
        }
    }
    
    let mut copied_size: u64 = 0;
    
    for (i, file_path) in file_paths.iter().enumerate() {
        let src_file = Path::new(file_path);
        let file_size = file_sizes[i];
        
        if !src_file.exists() {
            window.emit("import-progress", &format!("Skipped: {} (file not found)", file_path)).map_err(|e| e.to_string())?;
            continue;
        }
        
        let file_name = src_file.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
        let target_file = dest.join(file_name);
        
        // Skip if file already exists
        if target_file.exists() {
            window.emit("import-progress", &format!("Skipped: {} (already exists)", file_name)).map_err(|e| e.to_string())?;
            copied_size += file_size; // Count as "copied" for progress calculation
            window.emit("import-progress", &format!("PROGRESS_BYTES:{}:{}", copied_size, total_size)).map_err(|e| e.to_string())?;
            continue;
        }
        
        window.emit("import-progress", &format!("Copying: {}", file_name)).map_err(|e| e.to_string())?;
        
        // Copy file with progress tracking for large files
        match copy_file_with_progress(src_file, &target_file, file_size, copied_size, total_size, &window) {
            Ok(_) => {
                copied_size += file_size;
                window.emit("import-progress", &format!("Copied: {}", file_name)).map_err(|e| e.to_string())?;
            }
            Err(e) => {
                window.emit("import-progress", &format!("Failed to copy {}: {}", file_name, e)).map_err(|e| e.to_string())?;
            }
        }
        
        // Report final progress for this file
        window.emit("import-progress", &format!("PROGRESS_BYTES:{}:{}", copied_size, total_size)).map_err(|e| e.to_string())?;
    }
    
    Ok(())
}

fn copy_file_with_progress(
    src: &Path,
    dest: &Path,
    file_size: u64,
    initial_copied: u64,
    total_size: u64,
    window: &tauri::Window,
) -> Result<u64, std::io::Error> {
    use std::io::{Read, Write};
    
    let mut src_file = fs::File::open(src)?;
    let mut dest_file = fs::File::create(dest)?;
    
    let mut buffer = [0; 64 * 1024]; // 64KB buffer
    let mut copied_this_file = 0u64;
    let mut last_progress_report = 0u64;
    
    loop {
        let bytes_read = src_file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        
        dest_file.write_all(&buffer[..bytes_read])?;
        copied_this_file += bytes_read as u64;
        
        // Report progress every 1MB or 10% of file, whichever is smaller
        let progress_interval = std::cmp::min(1024 * 1024, file_size / 10).max(64 * 1024);
        
        if copied_this_file - last_progress_report >= progress_interval || copied_this_file == file_size {
            let total_copied = initial_copied + copied_this_file;
            if let Err(_) = window.emit("import-progress", &format!("PROGRESS_BYTES:{}:{}", total_copied, total_size)) {
                // Continue even if progress reporting fails
            }
            last_progress_report = copied_this_file;
        }
    }
    
    dest_file.sync_all()?;
    Ok(copied_this_file)
}

#[tauri::command]
async fn import_media(
    source_path: String,
    target_path: String,
    window: tauri::Window,
) -> Result<(), String> {
    let src = Path::new(&source_path);
    let dest = Path::new(&target_path);
    
    // Create target directory if it doesn't exist
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    
    // Collect all media files recursively
    let media_extensions = ["mp4", "jpg", "jpeg", "png", "mov", "heic"];
    let mut media_files = Vec::new();
    
    for entry in WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                if media_extensions.contains(&ext.to_lowercase().as_str()) {
                    media_files.push(entry.path().to_owned());
                }
            }
        }
    }
    
    let total = media_files.len();
    for (i, file) in media_files.iter().enumerate() {
        let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
        let target_file = dest.join(file_name);
        
        // Skip if file already exists
        if target_file.exists() {
            continue;
        }
        
        // Copy file
        fs::copy(file, &target_file).map_err(|e| e.to_string())?;
        
        // Report progress
        window.emit("import-progress", &format!("Copied: {}", file_name)).map_err(|e| e.to_string())?;
        window.emit("import-progress", &format!("PROGRESS:{}:{}", i+1, total)).map_err(|e| e.to_string())?;
    }
    
    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[tauri::command]
fn get_file_thumbnail(file_path: String) -> Result<String, String> {
    let path = Path::new(&file_path);
    
    if !path.exists() {
        return Err("File does not exist".to_string());
    }
    
    // Use the thumbnails crate on non-Windows platforms
    let thumbnailer = Thumbnailer::new(250, 250);
    let thumb = thumbnailer
        .get(path)
        .map_err(|e| format!("Error generating thumbnail: {e}"))?;

    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);

    thumb.write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| format!("Error writing PNG: {e}"))?;

    let res_base64 = general_purpose::STANDARD.encode(&buf);
    
    // Determine MIME type based on extension
    let mime_type = get_mime_type(path);
    
    Ok(format!("data:{};base64,{}", mime_type, res_base64))
}

#[cfg(target_os = "windows")]
#[tauri::command]
fn get_file_thumbnail(file_path: String) -> Result<String, String> {
    let path = Path::new(&file_path);
    
    if !path.exists() {
        return Err("File does not exist".to_string());
    }
    
    // For Windows, use a simpler approach with just the image crate
    let img = match image::open(path) {
        Ok(img) => img,
        Err(e) => {
            // For files that can't be opened directly (like videos),
            // return a generic icon based on file type
            return generate_generic_thumbnail(path);
        }
    };
    
    // Resize the image to thumbnail size
    let thumbnail = img.thumbnail(250, 250);
    
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    
    thumbnail.write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| format!("Error writing PNG: {e}"))?;
    
    let res_base64 = general_purpose::STANDARD.encode(&buf);
    
    // Determine MIME type based on extension
    let mime_type = get_mime_type(path);
    
    Ok(format!("data:{};base64,{}", mime_type, res_base64))
}

#[cfg(target_os = "windows")]
fn generate_generic_thumbnail(path: &Path) -> Result<String, String> {
    // Create a simple colored rectangle based on file type
    let width = 250;
    let height = 250;
    let mut img = image::RgbaImage::new(width, height);
    
    // Fill with a color based on file extension
    let color = match path.extension().and_then(|e| e.to_str()) {
        Some("mp4") | Some("mov") | Some("avi") | Some("mkv") => [120, 120, 255, 255], // Blue for video
        Some("jpg") | Some("jpeg") | Some("png") | Some("gif") => [120, 255, 120, 255], // Green for images
        _ => [200, 200, 200, 255], // Gray for other types
    };
    
    for pixel in img.pixels_mut() {
        *pixel = image::Rgba(color);
    }
    
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut cursor, ImageFormat::Png)
        .map_err(|e| format!("Error writing PNG: {e}"))?;
    
    let res_base64 = general_purpose::STANDARD.encode(&buf);
    
    // Determine MIME type based on extension
    let mime_type = get_mime_type(path);
    
    Ok(format!("data:{};base64,{}", mime_type, res_base64))
}

fn get_mime_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("heic") => "image/heic",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("avi") => "video/x-msvideo",
        Some("mkv") => "video/x-matroska",
        _ => "application/octet-stream",
    }
}

#[tauri::command]
async fn save_destination_path(path: String, app: tauri::AppHandle) -> Result<(), String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    
    // Create app data directory if it doesn't exist
    if !app_data_dir.exists() {
        fs::create_dir_all(&app_data_dir).map_err(|e| e.to_string())?;
    }
    
    let config_file = app_data_dir.join("config.json");
    let config = serde_json::json!({
        "destination_path": path
    });
    
    fs::write(config_file, config.to_string()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn load_destination_path(app: tauri::AppHandle) -> Result<String, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let config_file = app_data_dir.join("config.json");
    
    if !config_file.exists() {
        return Ok(String::new());
    }
    
    let config_content = fs::read_to_string(config_file).map_err(|e| e.to_string())?;
    let config: serde_json::Value = serde_json::from_str(&config_content).map_err(|e| e.to_string())?;
    
    Ok(config["destination_path"].as_str().unwrap_or("").to_string())
}

#[tauri::command]
async fn open_destination_folder(path: String) -> Result<(), String> {
    let dest_path = Path::new(&path);
    
    if !dest_path.exists() {
        return Err("Destination folder does not exist".to_string());
    }
    
    let mut command = if cfg!(target_os = "linux") {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(&path);
        cmd
    } else if cfg!(target_os = "macos") {
        let mut cmd = Command::new("open");
        cmd.arg(&path);
        cmd
    } else if cfg!(target_os = "windows") {
        let mut cmd = Command::new("explorer");
        cmd.arg(&path);
        cmd
    } else {
        return Err("Unsupported platform".to_string());
    };

    command.output().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn check_files_exist_in_destination(
    file_paths: Vec<String>,
    destination_path: String,
) -> Result<Vec<bool>, String> {
    let dest = Path::new(&destination_path);
    let mut results = Vec::new();
    
    for file_path in file_paths {
        let src_file = Path::new(&file_path);
        let file_name = src_file.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
        let target_file = dest.join(file_name);
        results.push(target_file.exists());
    }
    
    Ok(results)
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            list_removable_drives,
            list_media_files,
            get_file_thumbnail,
            unmount_drive,
            import_selected_files,
            import_media,
            save_destination_path,
            load_destination_path,
            open_destination_folder,
            check_files_exist_in_destination
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
